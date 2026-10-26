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
accepted for implementation. Stage 5 measured the shipping implementation and
confirmed every threshold, with a confirmation run.

Stage 6 bulk membership: **Reported, 2026-10-26 UTC; current prototype rejected
for adoption by the reduced run's setup-p95 gate.** The owner approved the
20-case amendment and stopping on a clear gate failure. The run stopped after
65.045 seconds, with Q01–Q03 complete in Node; Q03 setup p95 failed. No large
case, small control, Chromium timing, allocation, retention or confirmation
phase was started in that run. This is not evidence against the API shape or
proof of the tail's cause. Shipping code and adoption remain unchanged.
Earlier inconclusive observations and all original reports remain preserved.
The later eight-minute amendment completed both phases in 245.385 seconds.
Its fresh-process setup-tail results are inconsistent (one of three processes
fails the p95 allowance). All three bulk diagnostics show the expected action
benefit, but append teardown is slower in the candidate. No candidate-specific
cause is established; these diagnostics do not change adoption or the original
failed gate. Allocation, retained memory and confirmation remain unmeasured.
The owner subsequently requested an implementation checkpoint. It is authored
at the explicit experimental package entry, with correctness/consumer tests;
the regular entry and measured sources are unchanged. The checkpoint-path
investigation reproduced and fixed redundant once-listener removal. Three
post-fix reproductions passed the abort gates. Admission then stopped at Q02's
setup-p95 gate after two Node cases; allocation, retention and confirmation
were not started, following the agreed rejection stop rule. The checkpoint
remains experimental and unaccepted, with the original sources preserved.
The owner then authorized memory evidence independently of setup admission.
That phase completed all selected cases inside the original sixty-minute
ceiling. Large allocation targets fit, the single-append small allocation gate
fails, and retained-heap acceptance remains unestablished. Setup-only profiles
do not resolve the tail. The regular collection/store received independent
once-listener fixes. None of this adopts bulk membership or starts confirmation.
The subsequent scoped-commit and diagnosis round preserved the measured
checkpoint in Git, corrected attributable single-insertion planning overhead,
and reproduced C02 within its unchanged allocation allowance. Separate
retention calibration still cannot resolve the 16 KiB budget reliably; the
experimental API remains unaccepted and no substantial rerun was started.
The approved cap amendment resolved the small-cell calibration in both engines.
A runner correction preserved an unnecessary extended-calibration attempt and
continued without resetting the original initial-set deadline. The 45-minute
ceiling stopped the set with 12 of 184 timing cells complete. Bulk action
medians show potential, but initial setup/tail concerns, missing comparisons,
confirmation, allocation and retention evidence prevent adoption. Shipping code
is unchanged. Earlier partial runs are retained below.

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

| Variant         | State                                                                                                      | Wiring                                                                                                                                                                                      |
| --------------- | ---------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0. Hand-written | A map of items and an array of ids                                                                         | Actions update the map and assign the row's inputs directly; one listener per row with the row's signal                                                                                     |
| 1. Lattice      | A tasks capability on `createStore`: `order`, an array of ids, and `byId`, items by id, replaced immutably | A collection binding watches `order` and reconciles; a row binding per item, in the bindings contract's shape, watches its item's selection and calls the label action from the row's event |

No new Lattice API is introduced; the capability, bindings and reconciler are
lab code on the store's public API.

### Workloads

| Workload           | Sequence                                                                             |
| ------------------ | ------------------------------------------------------------------------------------ |
| Mount              | Create the state, the rows and their wiring for N items, connected                   |
| Single-item update | Change one item's label, alternating between two values, on items chosen with seed 1 |
| Reorder            | Reverse the order of all items                                                       |
| Teardown           | Abort the collection's lifetime: every row lifetime ends and every row is removed    |

Sizes: 10, 100 and 1,000 items.

### Measures

| Measure                | Evidence                                                                                                                                                   |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Time per operation     | Observed: `performance.now()` around mount, teardown and reorder per item, and around 1,000 single-item updates per update, in cross-origin isolated pages |
| Allocations per update | Profiler: steady-state bytes per single-item update after 20,000 warmup updates, sampled at 32 bytes                                                       |
| Memory per item        | Profiler: JavaScript heap after garbage collection, mounted minus empty, divided by N                                                                      |
| Work counts            | Observed: selection reads, row input assignments, control and text writes, moves, and rows created and removed per operation                               |
| Cleanup                | Observed: observers disconnected, and DOM listener and node counts back to their baseline after teardown                                                   |

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

| Variant           | Shape                                                                                                                                                                                                                                                                                                                                                                                                                  |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1. Shared map     | One store holding `order` and `byId`, replaced immutably. A collection-wide selection, the number of open items, declares `order` and `byId`                                                                                                                                                                                                                                                                           |
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

### Stage 6: bulk membership (2026-10-25, plan awaiting approval)

**Question and scope.** Can one explicit bulk membership operation eliminate
repeated whole-order copies while leaving small existing operations cheap?
This continues the collection question with the current shipping collection
as control, rather than carrying forward stage 5's performance conclusions.
The proposed behavior is owned by the
[bulk membership proposal](../specs/contracts/collections.md#proposal-bulk-membership).
Its method name, anchor restriction and thresholds below are all proposed.

**Owner amendment, 2026-10-25, recorded before measurements.** The owner
approved `updateMembership({ remove, insert })` for evaluation, including
surviving-key anchors, stable insertion order and unchanged existing batch-read
semantics. Apply the 2× speedup and 65% allocation-reduction gates only to
membership-only bulk workloads. For mixed actions containing item updates and
a full reorder, require no meaningful total-action regression: use the
recorded non-bulk p50 allowance (the larger of 5% or 0.5 µs) and the recorded
p95 allowance (the larger of 10% or 2 µs). Report mixed-action improvement
without requiring unrelated work to be halved. All correctness, identity,
rollback, cleanup, small-operation, lifetime-allocation and retention gates
remain. The run budget is a ceiling. Complete smoke and calibration first;
stop early on correctness failure or if calibration cannot resolve the
thresholds. No public API adoption or shipping implementation is authorized.
This amendment supersedes the mixed-workload speedup/allocation requirement
and the original paragraph treating every workload as a speedup gate below;
the original proposal is retained as the pre-approval plan.

**Hypothesis.** Source inspection shows `insert` spreads the complete keys
array and `remove` filters it on every call, including inside `batch`.
For C membership changes over N items this can copy O(NC + C²) keys.
A supplied atomic change set can build one next order in O(N + C), plus item
record copying. This is an algorithmic prediction, not a timing or allocation
measurement. Lazy publication for existing operations is rejected because
reads inside a batch must observe each pending write; a linked mutable order
would also impose new bookkeeping on every collection.

**Variants.**

| Variant | Implementation and supported scope |
| --- | --- |
| B0, control | Current `@canonic2/lattice/collection`, unchanged, using repeated `remove` and `insert` inside one `batch`; existing item updates and reorder stay in the same batch where requested |
| B1, candidate | Local copy of the same module with only `updateMembership` added, following the proposal: prevalidated removal set and insertion buckets, one final frozen keys array and existing undo/commit/delivery paths; complete proposed semantics, no alternate subscription strategy |
| D0, diagnostic only | Build the same final plain keys array from the normalized changes without a collection, records, rollback or subscriptions; estimates the order-building floor and cannot be adopted |

B0 and B1 import through equivalent explicit module entry points, including
their real initialization. B0 source stays read-only. B1 and runners stay
local and ignored. No production instrumentation or duplicate shipping backend.
First inspect any retained local runner for reuse; previous runners do not
establish these workloads or correctness. No reusable fixtures directory exists
at planning time; this stage's deterministic generator remains local.

**Inputs.** Use keys `t0` through `t(N-1)` and records
`{ label: 'Task i', done: i % 3 === 0 }`. A seeded 32-bit xorshift generator
(seed 1; shifts 13, 17, 5) selects distinct scattered indices by Fisher–Yates
shuffle. Use N = 100, 1,000 and 10,000. For each N use C = 1, 10,
ceil(N/10) and N, deduplicating identical sizes. Include an extra N = 10,
C = 0, 1 and 2 matrix to expose small-operation overhead. Every variant uses
the identical canonical fixture and expected output.

| Workload | One logical action |
| --- | --- |
| Append | Insert C fresh keys at the end |
| Prepend | Insert C fresh keys before the first original key, preserving supplied insertion order |
| Scattered insertion | Insert C fresh keys distributed over original anchors chosen by the seeded sequence, with multiple entries per anchor and append entries |
| Scattered removal | Remove C distinct original keys in seeded order, including complete removal at C = N |
| Replacement | Remove C original keys and insert new records under those same keys in their original positions; anchor each run before its next surviving key, or append when none survives |
| Mixed batch | Remove C original keys and insert C fresh keys before surviving anchors or at the end, update `done` and `label` on min(C, N-C) survivors, then reverse the complete resulting order, all in one outer batch |

For replacement, insertion tuples are in original order; removal order remains
seeded. For mixed batches, field updates precede membership, and reorder follows
it. The B0 adapter performs all supplied removals then insertions in their
supplied orders. B1 uses one bulk call inside the same outer batch. Also record
standalone single `insert` and `remove` on B0/B1, plus empty change sets and
item-only updates, to detect costs imposed on paths that do not use the API.

Run two subscription modes: none, and a representative mode with one item
watcher per original item plus one membership watcher and one collection-wide
membership-count selection (`fields: []`, reading `size`). Item watchers read
`label` and `done`. Register replacement watchers only after the action; they
must receive only their initial delivery. DOM and presentation are excluded
because this decision concerns the pure collection. Setup is timed separately.

**Expected results and correctness.** A straightforward array/map oracle
applies removals and sequential insertions outside timed regions. Compare exact
order, item values and survivor identities after every action. Compare
membership metadata and all delivery counts with B0, including initial calls
separately; each affected subscription is evaluated at most once per commit.
Mixed actions must expose no partial result to callbacks. Replacement must
deliver membership metadata even when key strings and order match.

Every sample also runs a separate untimed semantic sequence against each
variant: retained frozen arrays across successive reads/writes; reads inside
ordinary batches between existing operations; bulk calls before/after ordinary
writes; nested successful savepoints and caught inner failures; invalid input
and throwing getters after an earlier outer write; rollback after replacement;
insert/remove transient items; abort during a rolled-back removal; cleanup of
removed subscriptions and live signal listeners; new watchers skipping earlier
queued commits; reentrant membership and item writes; initial and delivery
errors with the existing reporter rules. B1 additionally rejects removed or
new insertion anchors without partial mutation. Weak references and forced GC
in separate retention samples check released keys, records and callback
captures after both commit and rollback; retained fixtures must not keep them
alive accidentally. A failed semantic comparison rejects the candidate.

**Timing boundaries.** Primary metric: total synchronous elapsed time from
logical-action entry through final commit delivery, including conversion from
the canonical workload into API input and input validation. Collection setup,
subscriber setup, fixture generation, oracle evaluation and teardown are
outside that boundary and reported separately. Also time the API call alone
with prebuilt input as a diagnostic; it cannot replace total-action time.
Use monotonic `performance.now()`. No profiler, counters or forced GC runs
inside timing samples. Fresh fixtures prevent growth across repetitions;
keep construction outside the timer but report the complete create/action/
abort lifetime as its own measurement. Teardown aborts every owned signal and
drops the collection; there is no public collection disposal method.

**Calibration and repetitions.** Run only after agreement. First smoke every
workload group with B0/B1, one sample per size, with full semantic comparisons.
Then calibrate timer overhead and repeat counts using the N = 10 and 100
small-operation cells. Accumulate separately timed chunks of independent
prepared collections until their summed action time spans at least 20 ms per
block; reset fixtures outside the timed chunks. Cap a block at 10,000 actions
and simultaneously prepared live data at 100,000 items. If the cap prevents
reliable resolution, label the
cell unresolved instead of stretching the budget. Fix repetition counts before
the main set. Each cell takes five warmup blocks and 15 measured blocks;
alternate B0/B1 order using seed 1. A second independent set repeats decisive
cells: all workloads at N = 1,000/10,000 with C = ceil(N/10) or N, and all
small/non-bulk controls. Report n, p50, p95, max and IQR, including absolute
differences; p95 with 15 samples is exploratory. Compare per-cell paired ratios
and both run sets, without pooling away losing cells.

**Allocations and retained memory.** In separate Chromium CDP sessions use
HeapProfiler allocation sampling at 512 bytes with collected objects included.
Measure action-only and total create/action/abort lifetime in separate samples;
report estimated allocated bytes/action, allocation-site attribution and sample
counts, never call sampled bytes exact total allocations. Five independent
profiles per cell run at least 1,000 actions or 1,000,000 supplied changes,
whichever comes first, with an identically prepared empty-loop control.
For action-only profiling, start sampling after preparing each bounded chunk
and stop before teardown; combine attributed samples across chunks. Lifetime
profiles include those phases. D0 runs only in the no-subscription timing and
allocation cells at N = 1,000/10,000 and C at least ceil(N/10); it has no
contract or retained-subscription result.
Do not subtract profiler overhead from timing results. If sampling cannot
resolve a comparison, mark it inconclusive and propose a method amendment
before further profiling. This does not authorize an unplanned profiling run.

For retained heap, collect five independent samples per cell after explicit GC:
empty realm; initialized collection and subscribers; completed action with live
collection; and aborted/dropped collection after commit or rollback. Drop
temporary inputs, oracle results, callback histories and old snapshots before
the post-action snapshot. Also measure caller-retained original keys separately
so required snapshot retention is visible rather than mistaken for a leak.
Report baseline-adjusted live bytes, bytes/item and post-teardown growth. Record
forced-GC convergence; failure to converge is unresolved, not zero retention.

**Decision rule: approved with the owner amendment above.** All correctness requirements must
hold. Keep the simpler implementation on an inconclusive result.

1. For each nonempty bulk workload at N = 1,000 and 10,000 with C at least
   ceil(N/10), B1 total-action p50 must be at most 50% of B0 in both timing
   sets. The gain must exceed both variants' IQRs. Total-action p95 must not
   regress beyond the larger of 10% or 2 µs in any cell.
2. At those large bulk cells, estimated action allocations must be at most
   35% of B0. Report lifetime allocations as well; they must not regress by
   more than 10%. Every small bulk cell must stay within the larger of 25%
   or 2 µs of B0 p50, and within 256 additional sampled bytes/action.
3. Existing non-bulk controls, setup and teardown must stay within the larger
   of 5% or 0.5 µs of B0 p50; no meaningful p95 regression beyond the larger
   of 10% or 2 µs. Live retained overhead must be at most the larger of 10%
   or 16 KiB over B0 per collection; post-teardown growth must be at most
   16 KiB over its empty baseline and show no growth over 100 churn cycles.
4. Counted diagnostic runs must show one new exposed keys array per nonempty
   bulk call and no extra subscriber evaluations or deliveries. Source review
   must establish one original-order traversal and O(C) bucket traversal, plus
   supplied record-field copying; timing slopes alone do not prove complexity.
   Rollback must use the saved keys identity and O(C) undo entries. Report
   module bytes and added source lines as costs, without a fabricated size win.

These are per-cell gates, not averages. A mixed workload that fails because
of unchanged item-update/reorder work still fails the proposed rule: report it
and seek an explicit decision rather than excluding that work retrospectively.

**Host, tools and budget.** Node 24.12+ for pure timing and semantics; Chromium
from this package's pinned Playwright for confirmation, allocations and heap.
Before runs record actual versions, OS/architecture, CPU/memory, power and
background load; record the public repository revision and hashes of every
uncommitted measured source and runner. Existing headless timing numbers do
not establish the result on the current source or host. Use local modules and
installed tools only, no network, downloads, product data or external calls.
Each run owns and closes its browser contexts, server and child processes on
success, failure and cancellation. Request host access for loopback/browser
process needs. Run sequentially, with no other heavy suite deliberately running.
Proposed budget: 45 minutes per timing set and 60 minutes total for profiling;
stop and report unfinished cells at the limit, without dropping them from the
decision rule. Agreement authorizes these lab-specific comparisons and their
semantic assertions only, not package checks or unrelated heavy runs.

**Confirmation and integration.** Run decisive B0/B1 cells in both Node and
Chromium; an engine-specific win is scoped as such. After the report, obtain
the owner's adoption decision before changing product code. If accepted,
implement the chosen API with behavioral tests and synchronized architecture,
example and guide changes. Repeat the decisive sequences against the shipping
`@canonic2/lattice/collection` export, including correctness, allocation and
retention cells that determined adoption. A prototype win is not shipping
acceptance. Checks remain subject to the workspace's explicit-request policy.

**Follow-up calibration proposal, 2026-10-26 — not approved.** After the early
stop reported below, propose increasing the action cap only for cheap controls
and membership cells with C at most 1, from 10,000 to 100,000 actions per
block. Keep the 20 ms accumulated action-time minimum, the 100,000-item live
preparation cap, all performance/correctness gates and the original wall-time
ceilings. This changes the run procedure, not the product acceptance limits.
The current runner has not been changed or rerun under this proposal. After
agreement, recalibrate both variants and engines before deciding repetition
counts or launching any main set. Stop again if the amended cap cannot meet
the timing boundary. The observed single calibration block is not used to
choose a winner or substitute for the required repeated samples.

**Owner calibration amendment, 2026-10-26, recorded before continuing.** The
owner approved a 100,000-action cap only for cheap controls and single-change
calibration cells. All other cells retain the 10,000-action cap. Keep the
100,000-live-item preparation limit, original time ceilings and every
acceptance gate. Prepare bounded chunks outside the timed regions, with
identical boundaries for control and candidate. Stop accumulating after the
chunk that reaches 20 ms; do not consume the remaining action allowance.
If the amended cap still cannot resolve a cell reliably, report inconclusive.
The owner authorized continuing within this amended plan without another
approval. This supersedes the preceding unapproved proposal; API adoption and
shipping implementation remain separate decisions.

**Runner preparation, 2026-10-26, before the main sets.** The amended small-cell
calibration reached the duration boundary in Node and Chromium. Before fixing
main-set repetition counts, calibrate the remaining matrix cells with the same
approved per-cell caps; use the larger B0/B1 repetition count for both variants
of each paired cell, then freeze those counts. Oracle results are computed
once outside timing for each deterministic cell and checked against each
fresh fixture after timing. Separate collection construction and subscription
setup counters are outside action timing. Chunks account for inserted items
as well as initial items so simultaneously prepared live records stay within
100,000. These runner details do not change fixtures, logical actions, gates
or budgets. Each timing set has a 45-minute wall-clock deadline, including its
preparation; incomplete cells remain explicitly unmeasured. The read-only
power-state probe was blocked by workspace command policy; record power state
as unavailable rather than substituting a different command.

**Budget enforcement clarification, 2026-10-26, during full-matrix calibration.**
The 10,000-item subscribed preparation is synchronous and can hold the driver
inside one cell for a long period. Add an external, low-frequency watchdog
against the original run start (no budget reset), which verifies the owned
process identity and enforces the same 45-minute deadline even while JavaScript
is inside that cell. Preserve completed cells and mark any interrupted cell
unmeasured. This changes no candidate, workload, metric or gate. No benchmark,
profiler or package suite runs alongside the active calibration.

**Runner correction, 2026-10-26, before main measurements.** The extension to
fresh-fixture calibration of every large subscribed cell was a runner error:
it repeatedly measured the cost of preparing and releasing all subscriptions
before obtaining a few microseconds of write timing. Preserve that partial run
as `runner-corrected`, not a candidate failure; do not erase its data. The
approved small-cell calibration in both engines already resolved with the
amended caps.

For steady-state action timing, prepare one warmed fixture per variant and
restore its logical input state outside each timed action using public
operations. Removed original incarnations receive fresh subscriptions during
restoration; all prior subscriptions end normally. Compare the same actions,
values, survivor identities and delivery counts, and keep input conversion,
validation and commit delivery inside the action timer. Validate fresh
fixtures separately. Prepare and restore bounded data outside timing, apply
the same boundaries to both variants, and retain the 100,000-live-item bound.
Calibrate with the same 20 ms target and amended per-cell caps, then freeze
paired counts. This avoids multiplying full subscription construction by the
steady-state repetition count.

Measure fresh setup, teardown and complete create/action/abort lifetimes
separately, at their own repetition count rather than multiplying those cold
phases by the steady-state count. Keep the required five warmups and fifteen
measured samples. Retention/cleanup probes and the fresh-fixture oracle cases
remain required; this is no relaxation of those gates. Use seed 1 to shuffle
the full matrix before the first main timing so a time-limited partial report
does not always omit the largest sizes. The corrected initial set keeps the
original run deadline, with no budget reset; stop unfinished cells at that
deadline. Prototype code and every acceptance threshold remain unchanged.

### Stage 6: proposed decisive comparison amendment (2026-10-26 UTC)

**Status: Approved by the owner on 2026-10-26 UTC; run reported below.**
The owner approved all 20 cases, independent large-case repetition counts and
reduced allocation/retention sampling. Setup, resets, correctness checks and
counters stay outside action timers; setup is measured separately. Starting
states and logical work must match, with results normalized per action. The
two-minute diagnostics are investigation limits, not grounds to dismiss a
regression. Five allocation profiles are initial evidence; unresolved variation
means inconclusive. Use the initial comparison to decide whether confirmation
is warranted: stop on a clear adoption-gate regression rather than confirming
a rejected candidate. Preserve all earlier evidence and numerical gates;
deferred cells remain unmeasured. No further pause is needed within this plan.

This is a reduced matrix and a bounded measurement procedure. It
does not change the prototype, adopt an API, relax any numerical acceptance
limit, replace the preserved partial reports or begin another roadmap stage.

#### Cases fixed before running

A case is one workload/size/subscription condition, comparing B0 with B1.
Run these **20 paired cases** in Node and installed Chromium. S means the same
representative subscriptions as before: one label/done item watcher per
original item, one membership watcher and one membership-count watcher.
U means no subscriptions. C is the number of keys affected, as in the earlier
generator; replacement has C removals and C insertions.

| IDs | N | C | Workload | Mode | Paired cases | Why retained |
| --- | --- | --- | --- | --- | --- | --- |
| B01–B02 | 1,000; 10,000 | N | Append | S | 2 | Growth to twice the original size; bulk construction and order copying |
| B03–B04 | 1,000; 10,000 | N | Seeded scattered removal | S | 2 | Shrink to empty, removal cleanup and scaling |
| B05–B06 | 1,000; 10,000 | N | Same-key replacement in original order | S | 2 | Incarnation replacement even when final key strings/order match |
| B07 | 1,000 | 1,000 | Seeded scattered removal | U | 1 | Preserve the old unfavorable control-IQR/noise case |
| B08 | 10,000 | 1,000 | Seeded scattered removal | S | 1 | Pure membership with surviving records/order at the 10% bulk boundary |
| B09 | 10,000 | 1,000 | Mixed update/remove/insert/reverse | S | 1 | Genuine mixed work: 1,000 survivors updated, 1,000 removals/insertions, full reorder |
| B10 | 10,000 | 10,000 | Full mixed replacement/reverse | S | 1 | Preserve the expensive old sentinel; explicitly has zero survivor updates |
| Q01 | 1,000 | 10 | Scattered removal | U | 1 | Exact prior setup-regression condition; full case remains gated |
| Q02 | 100 | 1 | Mixed update/remove/insert/reverse | S | 1 | Exact prior p95 concern; full case remains gated |
| Q03 | 100 | 1 | Scattered removal | S | 1 | Preserve the interrupted cell as a named pending comparison |
| C01 | 10 | 0 | Empty bulk change | S | 1 | One canonical no-op, merging the two old equivalent empty aliases |
| C02 | 10 | 1 | Bulk append | S | 1 | Cost of the new API for one change |
| C03 | 10 | 2 | Bulk prepend | S | 1 | Preserve the old small two-insertion comparison and stable anchor ordering |
| C04–C07 | 10 | — | Existing empty batch, single insert, single remove, item-only update | S | 4 | Protect ordinary paths, including a true existing-batch control in both variants |

Use the existing seed-1 generator, records, insertion anchors and expected
results. C04 calls ordinary batch in both variants; it is distinct from C01's
comparison with the proposed bulk API. Initial subscription delivery is outside
action timing and counted separately in correctness runs.

This reduces 184 paired cases to 20, without deleting any old result. It
retains the exact setup and p95 concern cases, the noisy full-removal case and
the interrupted case. Pure prepend/scattered-insert combinations, the other
no-subscription bulk combinations and the 100-item append/prepend/ten-key
replacement/scattered-insert performance cells are **deferred, not passing**.
Their functional anchor/order cases remain in untimed correctness probes.
The two old empty aliases are merged because they supply the same empty API
change set, not because either result is unfavorable. Do not extrapolate a
successful reduced matrix into acceptance of every deferred size/change/mode.

B07 retains the noisy workload but its proposed fresh-fixture condition is a
scope change, not a retrospective cure for the old warmed-run IQR result.
Report those conditions separately. Q02 retains the warmed condition and adds
an instrumented diagnostic; Q01 retains the original fresh setup boundary.

#### Runner boundaries and readiness

Keep the recorded B0 control and B1 candidate hashes fixed. B0 uses the
shipping collection entry point read-only; B1 remains a local prototype.
Readiness smoke must exercise both variants for every selected group, timing,
allocation and retention boundaries, cancellation, failure and cleanup before
a long set. Runner defects stop that set and receive a dated correction;
candidate correctness failure rejects the candidate rather than being tuned
away.

**Clean decision timing.** Disable subscription counters, metadata histories,
profilers and GC observers in every acceptance timing. Keep deterministic
counted runs separate. Use identical declared fields, readers, equality,
callback roles and error policy in B0/B1. The clock starts before API input
conversion and ends after the synchronous commit has delivered. No input
adaptation or validation is shifted out of that boundary.
This applies to subscriptions installed by restoration/rebinding as well as
initial setup; merely changing the initial counted flag is insufficient.
Run the full semantic stress probes in a separate owned validation process or
realm, serialized before timing blocks. They remain required, but must not
feed different object shapes or extra allocation history into the two timing
processes. Keep canonical per-action value/identity/error validation outside
each timer in the timing process itself. This isolation is an explicit runner
amendment; its cost remains in the phase wall budget.

**Large cases B01–B10.** Prepare fresh bounded fixtures outside the action
timer and run one logical action per fixture. Module code is warmed, but the
collection/registration state is fresh. Drop the fixture after post-timer
validation and cleanup; do not restore thousands of keys using repeated
scalar operations. This also restores canonical subscription registration
order on every action. These are fresh-collection action measurements; do not
pool them with the earlier warmed/restored action samples.

Calibrate each large variant independently to 20 ms, with at most 10,000
actions, stopping at the first completed bounded chunk that reaches the
target. Freeze each variant's count before its decision blocks and reuse it
in confirmation. Different repetition counts are an **explicit amendment**:
every invocation has the same measurement boundary and logical workload,
and every variant still has five warmups and fifteen measured blocks.
A slow four-second control is not repeated dozens of times solely because
the candidate needs several calls to accumulate 20 ms. Normalize per call;
report counts and unnormalized block durations too.

**Concern/small cases Q01–Q03 and C01–C07.** Use prepared, restored fixtures
for steady-state action comparisons, with reset outside timing. Keep matched
B0/B1 repetition counts for these small cases. The 100,000-action cap applies
only to cheap controls and C <= 1 calibration; all other cells keep 10,000.
The 20 ms target and 100,000-live-item preparation limit stay unchanged.
Use indexed structural checks and lazy failure diagnostics outside the timer
instead of constructing full reference maps and eager JSON diagnostic strings
on every action. Every action still checks exact values/order, survivor and
keys identities, and errors. Counted preflights additionally check callback
history, ordering, incarnations, abort and at-most-once delivery.

**Cold phases.** Retain the original complete input-generation/create/
subscribe/action/abort boundary and separate construction, subscription and
teardown metrics. Use matched cold counts: 100 lifetimes/block at N <= 100,
five at N = 1,000, one at N = 10,000. For large fresh cases reuse the first
matched cold-count invocations of each action block; add only the required
missing cold invocations when a variant's action count is smaller. Label
those additional calls cold-only, not extra action samples. This avoids
performing a second expensive full action just to measure its lifetime.

After every block persist raw samples and phase wall times; never wait for a
whole cell to finish before saving. Separately account for fixture generation,
construction, subscription setup, action, validation, reset, teardown,
reporting and unclassified time. Verify conservation of the wall-time ledger.
An independent owned watchdog enforces the phase deadline during synchronous
work. A timeout is incomplete evidence, never a latency sample.

#### Bounded investigation of the concerns

Run these first, within a combined **four-minute ceiling**, charged to the
initial timing-set budget:

- **Setup, at most two minutes.** Repeat exact Q01 end-to-end setup with the
  old boundary and matched counts in both variants. In separate diagnostic
  groups measure fixture-input/controller generation alone and construction
  from prebuilt entries. Use three isolated preconditioning regimes: identical
  canonical-record warmup; identical benign empty/getter payload-shape warmup
  in both variants; and reproduction of the old variant-specific semantic
  prefix. Each has five warmup and fifteen measured blocks of 100 constructions
  in seeded paired order, split into bounded chunks. B1's extra old probes
  exercise getter/empty-record inputs through helpers also used by construction;
  unequal JIT/heap conditioning is a hypothesis to distinguish from an actual
  constructor regression, not an explanation established by source alone.
  Report pure-construction and common-harness costs without subtracting
  them from the primary setup result. This distinguishes constructor cost
  from generator/harness/JIT/host variation. Keep the old 378.547/423.309 µs
  setup result visible; it was collected without counted subscriptions.
- **p95, at most two minutes.** Repeat exact Q02 with counters off as the
  decision condition. Compare counters on/off and subscribed/unsubscribed
  versions as separate diagnostic conditions, with seeded paired order,
  five warmups and fifteen measured blocks. These extra diagnostic modes
  cannot become substitutes for Q02. Collect individual operation durations
  into preallocated buffers outside each action's timestamp boundary, while
  preserving block-mean p95 as the original acceptance statistic. Record
  timer resolution and host load. A short separate GC-attribution pass may
  run for at most ten seconds per variant; no tracer or GC observer runs
  during acceptance timings.

The p95 work diagnoses instrumentation, reset/heap history and runtime
variation; none is a presumed cause. Do not exclude outliers or GC-affected
decision blocks, pool away a bad replicate, substitute medians for p95, or
waive a reproduced regression. If the diagnostic ceiling prevents resolution,
report the concern as unresolved and preserve its failed/partial evidence.

#### Allocation, retention and confirmation

After the investigations and initial decision set, collect separate Chromium
memory evidence for the same 20 cases, then repeat all 20 timing cases in an
independent confirmation set in both engines. Keep Node/Chromium conclusions
separate. No product code changes or candidate tuning between these phases.

The old allocation-count heuristic is not affordable at the largest sizes:
a control costing about 4.6 seconds would require over seven minutes for one
100-action profile, before five replicates or lifecycle profiles. Therefore
the following sampling reductions are **proposed amendments, not accepted
results or changed performance limits**:

- Keep five independent profiles per case/variant and 512-byte allocation
  sampling with both minor- and major-collected objects included. For B01–B10
  take one complete action per profile, on independently initialized fixtures;
  one action already creates many order-array allocation samples in the
  control. Report sample counts and uncertainty; if insufficient, mark
  inconclusive rather than reporting zero allocation or increasing work
  unplanned. For Q/C cases keep 1,000 actions/profile in bounded prepared
  chunks. Warm Q/C fixtures and reset before sampling, never inside the
  action-only sampling window. Use identical empty-loop controls to expose
  profiler/harness attribution.
- Profile complete create/action/abort lifetimes separately, with the same
  large/small action counts. Keep fixture/oracle diagnostics outside sampling;
  preserve input generation inside the complete-lifetime boundary. Do not
  confuse sampled allocated bytes with retained heap or exact byte totals.
- Keep five independent retained-heap samples per case/variant for commit and
  rollback: empty, initialized, completed with live collection, and
  aborted/dropped collection. Drop runner inputs, oracle references and
  histories before post-action heap measurements. Measure caller-held old
  keys separately at B06 and C02, in five samples each, rather than retaining
  old arrays accidentally in every sample.
- Keep 100-cycle churn at B06 (10,000-item same-key replacement, subscribed)
  and Q02 (partial mixed state, subscribed). These exercise maximal
  incarnation/signal cleanup and touched-record/batch bookkeeping respectively.
  The other selected cases get the five commit/rollback retention samples,
  but are not individually claimed to have passed 100-cycle churn. Churn
  coverage is explicitly reduced, not its cycle count or retained-growth gate.
- Record up to five forced-GC observations at each retained snapshot. Keep all
  observations. If their range can change the memory-gate outcome or weak
  references remain live for reasons the probe cannot resolve, report
  inconclusive. Do not choose a favorable minimum heap value.

Correctness, 2× membership-only speedup, 65% allocation reduction, mixed-action
non-regression, small-operation limits, setup/teardown limits, live/post-drop
memory limits and meaningful-difference rules remain exactly as agreed.
Diagnostics never override a failing decision case. Confirmation uses the
same cases, scopes, source, boundaries and frozen repetition counts and
reports each set separately. All required selected gates must hold before an
adoption recommendation. Deferred cells do not become passed gates.
Any eventual adoption remains a separate owner decision, followed by behavioral
tests and decisive shipping-path comparisons; this plan does not authorize it.

#### Cost estimate and ceilings

These are planning estimates from preserved costs, not new measurements:

- The old 10,000-item full mixed cell has a fresh control action median of
  4.584 s and a complete control lifetime median of 5.239 s; B1's complete
  lifetime median is 0.948 s. Twenty matched fresh rounds therefore cost
  about 124 s per engine for this sentinel, before preflight/calibration,
  instead of paying quadratic reset plus a second lifetime pass. The old
  control lifetime p95 was about 30 s, so this is not a guaranteed bound.
- Full 1,000-item removal's fresh control action median is 37.81 ms.
  Scaling its visited-key count predicts roughly 3.8 s for full 10,000-item
  removal. The preserved 100-item append cost scales to roughly 0.9 s at
  10,000 full appends; full replacement is estimated at 4–6 s from removal
  work and the old full-replacement sentinel. Those are inferred estimates
  for unmeasured cases, not performance claims.
- Six subscribed bulk endpoints, the two partial cases, noisy-removal
  recheck and full sentinel should cost roughly 8–12 minutes per engine
  with independent large-case counts and no quadratic reset. Small cases,
  diagnostics, preflight, validation and reporting add roughly 5–10 minutes
  across the initial set. Budget for Chromium using the Node cost envelope,
  not an assumed browser speedup.
- Memory sampling at one large action/profile gives approximately 4–6 minutes
  of unprofiled action/lifetime work; allow 12–22 minutes for allocation
  sampling and its controls. Five commit/rollback samples and the two
  caller-retention sentinels add roughly 5–10 minutes. B06's 100-cycle churn
  costs about ten minutes for the variant pair under the old lifetime
  envelope; allow 12–18 minutes including GC/driver overhead. The estimates
  are deliberately approximate because profiler and GC costs are unmeasured.

| Phase | Estimated duration | Unchanged hard ceiling |
| --- | --- | --- |
| Diagnostics, smoke, calibration and initial 20-case timing set, both engines | 25–35 minutes | 45 minutes total for this set |
| Allocation and retention, including selected churn | 30–50 minutes | 60 minutes total |
| Independent confirmation, same 20 cases in both engines | 20–30 minutes | 45 minutes total for this set |

Total estimate: **75–115 minutes**, versus the unchanged 150-minute aggregate
maximum of the three ceilings. Phase budgets are not transferable and do not
reset on a runner correction. Start each watchdog before smoke/preparation.
Record phase costs continuously so an estimate cannot silently become a new
allowance. Stop on correctness failure, unresolved calibration, prerequisite
failure or the phase ceiling; preserve partial work and label unfinished cases.
No additional matrix, candidate, sampling interval or threshold change is
implicitly authorized by approving this proposal. No network, downloads,
external data, other product edits or package checks are included.

### Stage 6: proposed setup-tail and bulk diagnostic amendment (2026-10-26 UTC)

**Status: Approved by the owner on 2026-10-26 UTC; run reported below.** The
owner clarified that each fresh process runs both variants, interleaving
their blocks and alternating which runs first. Keep process results separate;
p95 denotes p95 of block-average setup times, not individual setup latency.
Proceed through both phases without pausing on a setup performance result.
Keep the
prototype unadopted, the measured control/candidate sources unchanged, all
original reports intact and every numerical adoption gate unchanged. This
amendment asks two questions: does Q03's setup tail repeat across fresh
processes, and do the intended large membership operations show the expected
action-time benefit? Bulk diagnostic results cannot override a failed setup
gate or establish adoption without the absent memory/confirmation evidence.

#### Fixed scope and process isolation

Use Node 24.12.0 on the same host, offline, without package checks, downloads
or product edits. This is a Node-only diagnostic amendment, not completion
of the earlier Node/Chromium matrix. Record source hashes, runtime, clock
resolution and host/load conditions before measurement. Preserve all raw
blocks in a new owned results directory. Save after every block and enforce
deadlines from a coordinator able to terminate its own synchronous children.

Before measurements, smoke the three bulk operation groups and the exact
Q03 setup/subscription lifecycle at small sizes in a separate validation
process. Keep semantic stress, counted delivery probes and correctness
checks outside timing processes or after action timers, as appropriate.
Check failure/cancellation cleanup before the bounded sets. Correctness,
ownership, source mismatch or runner failure stops the run. An observed
setup performance failure does **not** skip the three bulk diagnostics.

#### Setup tail: one case, three independent fresh processes

Use Q03 exactly: 100 initial items, one seeded scattered removal, one item
watcher per original item plus membership and size watchers. Measure fresh
setup separately from the removal. Its primary boundary remains input
generation, collection creation, owner/controller creation and subscription
registration; initial delivery is included. Abort, removal, resets, counters,
correctness checks and IPC stay outside the setup timestamp boundary.
Measure the complete teardown separately with the original abort boundary.
Keep the existing construction/subscription breakdown without subtracting
components from total setup. Identical starting values, order and declared
watcher roles are required for both variants.

Create **three sequential independent fresh OS processes**, each importing
both measured implementations and running paired B0/B1 blocks. Do not use
worker threads as a substitute for process isolation. Only one child executes
measurement work at a time. Strictly alternate which variant runs first on
successive paired blocks, with equal first positions over measured blocks;
alternate the initial first variant across processes. Close each process
before starting the next. No asymmetric
semantic preconditioning, forced GC, observers or profilers in this set.

Each process gets **20 warmup blocks and 200 measured blocks of 100 fresh
setups/teardowns**. Thus each variant has 200 block means and 20,000 setups
per independent process, with 60,000 measured setups across three processes.
This is fixed work: no calibration or repetition growth. At 200 blocks,
nearest-rank p95 is order statistic 190, rather than the maximum of fifteen.
Report every process separately: p50/p95/max/IQR for setup, construction,
subscription and teardown; retain paired differences and chronological
blocks. Do not pool processes into a single favorable percentile. Report
tail uncertainty using a seeded moving-block bootstrap (ten consecutive
blocks per resampling unit, 2,000 replicates), qualifying its limitations
for longer correlated pauses. Uncertainty does not change the acceptance
threshold or authorize extra samples.

References to a process pair below mean its paired B0/B1 results within one
fresh process, following the owner's clarification.

Use the unchanged setup/teardown allowances: p50 at most max(5%, 0.5 µs)
and p95 at most max(10%, 2 µs) worse than control. Report the numerical gate
outcome for each process pair. Call the setup-tail difference **repeated**
if the p95 allowance is exceeded in at least two of three pairs; report all
three, including discordant results and uncertainty. Call it not reproduced
under this process protocol only if all three fit the allowance; otherwise
call it inconsistent/unresolved. These are diagnostic repeatability labels,
not replacements for adoption gates or a retrospective pass of the failed run.

Reserve at most **30 seconds within the setup budget** for one additional
instrumented fresh-process pair, at most fifteen seconds per variant, on
this same case only. Capture GC events and CPU samples to relate pauses to
the existing construction/subscription/abort phases. These traces are
diagnostic and never enter primary percentiles. Attribute a regression only
when repeated primary excess, phase breakdown and trace evidence support
the same mechanism; temporal GC correlation or a sampled hot function alone
is not proof. If evidence is absent or conflicting, name the unresolved
cause rather than expanding into new component or workload matrices.

#### Bulk diagnostics: exactly three intended operations

Run these regardless of whether the setup allowance is exceeded, using
separate fresh OS processes from the setup investigation. Fix the order
below before running, with interleaved control/candidate blocks within each
case, both variants within each fresh case process and no concurrent
measurement work. Strictly alternate which variant runs first:

| Case | N | C | Subscriptions | Purpose |
| --- | --- | --- | --- | --- |
| B02 append | 10,000 | 10,000 | Same representative item/membership/size watchers | Grow to twice the initial size; avoid repeated order copying |
| B04 scattered removal | 10,000 | 10,000 | Same watchers | Remove all items; include commit cleanup |
| B06 same-key replacement | 10,000 | 10,000 | Same watchers | Preserve final key order while replacing every incarnation |

Use the existing seed-1 generator, input conversion and sequential oracle.
Each case gets **two warmup and seven measured paired blocks, one logical
action per variant per block**. Prepare a fresh fixture outside each action
timer; preserve equivalent values, order, subscription registration and
logical work in B0/B1. Start action timing before API-specific input conversion
and end after synchronous commit delivery. No reset, reference capture,
counters, setup or validation inside action timing. Validate exact values,
order, frozen/stable old keys, surviving identities, replacement incarnations
and errors after each action; counted preflights separately verify deliveries.
Abort every owned fixture and save setup/action/validation/teardown wall costs.
Keep at most one prepared fixture per variant; the 100,000-live-item ceiling
is unchanged. Do not use the prior fresh helper's capture-contaminated lifetime
span as a clean lifetime measurement; this set reports setup and action
separately and makes no complete-lifetime claim.

There is **no calibration**. One invocation is one action, normalized per
action identically for both variants. Report raw action durations, p50,
p95/max (seven samples make p95 the maximum), IQR, speedup and the previous
membership-only timing target: candidate p50 at most half control, with the
median gain exceeding both IQRs. Results are expressly **diagnostic**, with
no confirmation or allocation/retention acceptance claim. If a single call
is too short to resolve a difference, label it inconclusive; do not accumulate
more calls. Do not add mixed, prepend, small controls, other sizes, modes or
engines. Their existing evidence and deferred status remain unchanged.

#### Budget, stop conditions and report

| Phase | Estimate from preserved costs | Hard wall ceiling |
| --- | --- | --- |
| Readiness, setup-tail pairs and optional attribution | 30–90 seconds | 3 minutes total |
| Three bulk diagnostics, including preparation and cleanup | 2–4 minutes | 5 minutes total |

The estimate uses about 88 µs setup and 44 µs teardown per 100-item lifetime:
132,000 measured-plus-warmup lifetimes across three processes imply roughly
17 seconds before process/IPC/validation and tracing overhead. Bulk estimates
remain inferred: roughly 0.9 s for full append, 3.8 s for removal and 4–6 s
for replacement in the control. Nine rounds give roughly 78–96 seconds of
control action work, before candidate work, setup, smoke and cleanup. The old
30-second cold-lifetime tail makes those estimates uncertain; the watchdog
limits spending rather than extending the matrix to compensate.

Maximum is **eight minutes**, with no transfer or reset between phase ceilings.
The setup deadline stops that investigation, preserves incomplete evidence
and proceeds to the precommitted bulk diagnostics if correctness and ownership
remain sound. A bulk deadline preserves unfinished cases as unmeasured.
No source tuning, matrix expansion, repeated failed run, memory phase or
confirmation is included. Report setup repeatability per process, each bulk
operation's diagnostic benefit/uncertainty, any attributable regression and
the limits of that attribution. Keep adoption pending regardless of outcome.

### Stage 6: implementation checkpoint and proposed continuation (2026-10-26 UTC)

**Status: implementation checkpoint prepared; continuation authorized by the
owner's 2026-10-26 checkpoint instruction.**
The owner requested a reviewable implementation and correctness tests, preserving
normal shipping behavior and keeping acceptance pending. The actual opt-in
entry is `@canonic2/lattice/experimental/collection`; the regular export still
targets the unchanged accepted source. No root export or supported API guide
was extended. The unpublished package explicitly labels the opt-in experimental.

`src/modules/collection/experimental.js` initially ports the prototype's runtime
logic, with typed experimental membership changes, bounds annotations and a
relocated reporter import. Its initial hash is
`c45e032bca979be04c6f285ea0a44a24e81883af902614693a6eeaa45ede0f7d`;
the package manifest hash is
`fb72313f5a16fa52cac9ca79f8e015b26aa96ef221a9392a3772d4492b07e868`.
The core is separate so the accepted module, import graph, instance shape and
ordinary-call costs do not change during evaluation. Consolidation is an
acceptance/migration decision, not an implicit change in this checkpoint.

Correctness tests are authored beside the checkpoint, using its actual package
entry. Packed-consumer and Node import fixtures verify the regular collection
still lacks the experimental operation. Type fixtures cover item inference,
valid operations and invalid item/key/anchor/option calls in Node/browser type
environments. Their broader execution remains unverified. The lab's separate
correctness probes exercise the actual entry; authored test coverage is not a
completed package acceptance result.

#### Preservation and comparison inputs

The measured baseline, reporter and prototype were copied byte-for-byte into
a separate local frozen source tree with their original relative module
layout and a manifest, refusing to overwrite existing files. Their SHA-256
hashes remain the previously recorded baseline
`b2afe023ca3a1ebd81b741e6726273c97ce03a860702496fa70c8db928606f76`,
reporter `e4ad2ac9d60e185067e23107b1133b5f5067dbc16c5b4b9dc41401ce3ad2104d`
and prototype `22c3d04f416610f613ad383cd7f7e9738458adbf3ab339d19d69cf293aaeaacf`.
Original reports and their hashes remain untouched. The initial checkpoint
source/manifest are preserved separately before any corrective edit. Later
checkpoint revisions must get distinct hashes and saved reports; never replace
the original comparison source with a tuned checkpoint.

A local lab adapter now composes B0 from the frozen measured baseline and B1
from the actual experimental package export. It verifies all frozen source
hashes before entering the existing fixture/measurement protocol. Old runners,
prototype and reports are not rewritten. Browser comparisons must explicitly
resolve the manifest's experimental export to the same source bytes, recording
that mapping, rather than continue importing the original prototype.

#### Proposed bounded teardown attribution

This changes the attribution target from the old setup case to the observed
large-append abort regression, and changes B1 to the checkpoint package entry.
The owner's instruction explicitly requests this investigation against the
checkpoint and subsequent remaining measurements. The reproduction uses the
already selected workload, counts, fresh-process protocol and eight-minute
ceiling; separately instrumented diagnosis cannot change acceptance gates.
It introduces no new numerical acceptance threshold or claim of adoption.

Use Node 24.12.0 on the same host. Fixed case: 10,000 initial items, append
10,000, representative item/membership/size watchers; all original watchers
remain alive until abort. Compare B0 frozen baseline and B1 checkpoint in
three fresh OS processes, both variants interleaved within each, strictly
alternating which runs first. Two warmups and seven measured paired blocks
per process, one action per fixture, no calibration. Preserve equivalent
starting values, order, watcher registration and action. Prepare references,
run correctness checks and count deliveries outside action/abort timers.
Measure setup, append action, synchronous abort and validation separately.
Use no profiler, GC observer or forced GC in the primary set. Save each block,
report each process separately and retain every unfavorable sample.

Then take one separately instrumented paired append/abort sample through the
same entry points, starting CPU and GC observation only for attribution,
never using its timings as acceptance evidence. Take three additional paired
diagnostic samples with explicit pre-abort GC, alternating ordinary abort and
pre-GC abort within a fresh process to distinguish allocation/heap scheduling
from repeated release/unlink work. GC is outside abort timing; report its
duration separately. Do not subtract pauses from primary times or treat a
single profile/hot function as causal proof. No other workload, count, mode,
engine or forced-GC variation is included in this attribution amendment.

Keep all teardown limits: p50 at most max(5%, 0.5 µs), p95 at most
max(10%, 2 µs) above B0. Seven-sample p95 equals maximum and is qualified as
such. Repeatability requires the failed numerical gate in at least two of
three primary processes; inconsistent results remain unresolved, not passing.
Any identifiable mechanism must be supported by the repeated primary excess,
phase/profile evidence and the controlled diagnostic before changing code.

Estimate 3–6 minutes from the saved 4.572 s baseline append, roughly
0.7 s setup and 0.24/0.63 s abort costs. Hard ceiling **eight minutes**,
including readiness, source verification, preparation, attribution and cleanup.
One owned coordinator watchdog stops synchronous child work at the ceiling.
No calibration growth, additional cells or automatic rerun after a runner
correction. Timeout means incomplete evidence. Correctness/ownership/source
failure stops immediately. A performance failure remains evidence to explain,
not a reason to hide the intended comparison.

#### Corrections and remaining acceptance phases

**Checkpoint correction recorded before admission measurements.** The initial
checkpoint reproduced the large-append abort regression in all three fresh
processes (candidate medians 627.171/679.649/621.485 ms versus baseline
278.164/268.516/266.461 ms). CPU profiles place 564 of 577 candidate samples
in native `removeEventListener`, with zero observed GC during abort. Pre-abort
GC did not remove the excess. Once-listener dispatch has already removed the
current listener, but the release path redundantly searches the remaining
signal list for it. This is avoidable work shared with the old implementation;
the evidence identifies the dominant cleanup mechanism, not every reason its
cost differs between heap histories.

The experimental abort callback now clears its signal/onAbort registration
references before release. Other release paths still detach live owning
listeners. The ordinary module and frozen baseline/prototype remain unchanged.
Corrected checkpoint hash:
`7610e112f6a165f95bbcd13903c92d50e30853729499aca5a884926388405af1`.
The initial/corrected snapshots and separate reports were retained. The same
three-process reproduction then measured candidate abort medians
3.434/4.261/3.646 ms versus baseline 293.135/263.105/276.542 ms; every abort
p50/p95 gate fit. The candidate profile no longer has native listener removal
as its dominant sampled cost. Both reproduction sets completed within their
eight-minute per-revision ceilings; no runner correction reset a deadline.
Admission timing uses this corrected hash, not the original prototype.

Make only attributable corrections in the experimental module; never edit the
accepted module or frozen sources to improve the candidate result. Preserve
the initial checkpoint and every corrected revision. Add behavioral tests for
the mechanism and repeat the decisive teardown comparison through the actual
package entry with the same source/boundaries/counts per recorded revision.
An unresolved attribution is reported explicitly rather than prompting a
speculative optimization or erasing a gate.

The previously approved twenty-case matrix, independent large-case counts,
small/concern controls and numerical gates remain the acceptance scope. The
checkpoint has not completed that matrix, so do not relabel the three bulk
diagnostics as a complete initial comparison. After a justified correction,
the remaining evidence is: checkpoint initial twenty-case timings in Node and
Chromium (45-minute ceiling); the same twenty-case allocation/retention plan
(60-minute ceiling); independent twenty-case confirmation (45-minute ceiling).
Keep the approved five allocation profiles initially, one action/profile on
large cases, 512-byte sampling including collected objects, five retained
samples per case/variant for commit/rollback, caller-held keys at the named
sentinels and 100-cycle churn at B06/Q02. Unresolved variation is inconclusive,
with no automatic sampling increase. No deferred cell becomes passing.

Use the initial comparison to decide whether memory/confirmation is warranted;
do not spend those budgets on a clearly rejected candidate. Phase ceilings
are not transferable or reset by runner corrections. Any correction receives
its explicit comparison/revision record before rerunning; it does not silently
restart an exhausted phase. Source changes or further workload/sampling
amendments require agreement before measurements. The authoring request does
not adopt the checkpoint or publish bulk membership as supported.

### Stage 6: independent memory evidence and setup profiling (2026-10-26 UTC)

**Approved by the owner's 2026-10-26 follow-up instruction.** Allocation and
retention now proceed independently of the failed setup gate. Adoption remains
blocked, all original gates/evidence remain intact, and confirmation is not
authorized to override the failed gate. This explicitly supersedes the earlier
admission prerequisite for the memory phase only.

Use the unchanged twenty selected cases, frozen measured B0 and corrected
experimental package-path B1 (hash `7610e112f6a165f95bbcd13903c92d50e30853729499aca5a884926388405af1`).
The separately authorized cleanup correction in the regular collection/store
does not replace the frozen baseline. Preserve both current and frozen source
hashes in the report. No candidate tuning or matrix expansion during memory
measurement. Run profiled phases sequentially to avoid CPU interference.

Memory keeps its previously approved **60-minute combined ceiling**, five
independent allocation profiles per case/variant at 512-byte sampling including
minor-/major-collected objects, action-only and complete create/action/abort
lifetimes plus matched controls. Large cases get one action/profile; Q/C get
1,000 in bounded chunks with at most 100,000 simultaneously prepared items.
Five retained samples per variant/case cover commit and rollback with empty,
initialized, live-after-action and dropped snapshots. Keep all five forced-GC
observations at each snapshot; if their range can change a gate decision,
classify it inconclusive. Caller-held keys remain separate at B06/C02; B06/Q02
retain the two 100-cycle churn sentinels. Weak-reference release probes cover
commit and rollback. Insufficient sampling remains inconclusive, without adding
profiles or relaxing limits. An allocation/retention failure is reported but
does not silently suppress the other independent memory evidence; correctness,
ownership or deadline failure still stops the phase and preserves partial work.

Before that memory phase, investigate setup directly with source inspection
and a **three-minute ceiling** for separate setup-only CPU and allocation
profiles. Fixed case is Q02's 100-item subscribed initialization; no mixed action,
reset or abort inside the profiling window. Three fresh Node processes each
profile both variants in alternating first order, using 1,000 prepared setups
per profile and no more than 100,000 live items. Warm identical canonical input
shapes in each variant first. Separate CPU from allocation profiling; keep GC
events for correlation and record profiler overhead as diagnostic, not timing
acceptance. Source diff and sampled call/allocation sites distinguish collection
construction, record conversion, listener registration and initial reads.
The checkpoint's additional closure/property is a known source difference;
do not assume it explains the setup tail. This is not another identical
unprofiled timing run and cannot waive the original p95 failure. If attribution
is unresolved within the ceiling, preserve it and proceed with memory.

Regular collection/store once-listener fixes are a separate correctness change
under the owner's explicit instruction. The collection clears the already-ended
registration during its abort callback; the store avoids redundant removal when
an initial callback already aborted and deactivated its subscription before
throwing. Live-signal failure cleanup still detaches, callbacks remain ended,
and original errors propagate. Their regression coverage belongs with each core.
These independent fixes do not adopt bulk membership.

**Runner correction before completing small action profiles.** The first memory
coordinator prepared fresh Q/C action fixtures instead of warming and resetting
them as approved. Large fresh-case profiles and complete-lifetime profiles
retain their intended boundaries. Preserve every original row; classify the
affected small/concern action profiles as exploratory only, regardless of their
values. Warm, validate and reset Q/C fixtures outside sampling in the corrected
probe, including uncounted replacement subscriptions. The continuation reuses
all completed valid profiles by case/variant/replicate/scope/control key and
collects only the missing valid profiles. It writes a new report linked by
hash to the preserved partial report, with exactly the original sixty-minute
deadline. The old probe/coordinator bytes are archived and hash-verified.
Cancellation closed the owned browser, but the old coordinator awaited a stale
CDP promise; it was stopped after verifying no child processes remained, with
its pre-stop report preserved. The continuation races target operations against
cancellation so deadline/cleanup cannot leave that same stale wait. This is a
runner correction, not a threshold, sampling-count or budget amendment.

### Stage 6: single-append diagnosis and separate retention calibration (2026-10-26 UTC)

The owner requested scoped commits, profiling of the additional 375.871
bytes per single append, an attributable correction, and retention calibration
before another substantial run. The measured experimental bytes are preserved
in public commit `a92011d64fe6c979c9b6450b101d15daec8ac856`; independent cleanup,
regression coverage and findings are separate commits. Frozen B0/prototype and
earlier raw reports remain intact. The regular API is not changed by this work.

This is a bounded diagnosis, not a new adoption comparison. First profile C02
through the actual experimental package export, with the unchanged frozen B0:
five profiles per variant, 1,000 warmed/restored actions per profile, 512-byte
sampling including collected objects. Preserve full allocation stacks/lines.
Preparation, reset, validation and abort remain outside sampling, and each
chunk respects 100,000 live items. Alternate first variant by replicate.
Use a three-minute ceiling per before/after diagnostic, without adding samples
or extending the matrix. Inspect attribution before editing; after an
attributable correction, repeat the same probe. Keep the 256-byte allowance
and report observed ranges. This cannot clear other gates or authorize adoption.

Calibrate retention independently, with a separate three-minute ceiling and
no full matrix. Fixed shapes are C01's subscribed empty action at ten items
and a diagnostic subscribed full replacement at 1,000 items. Compare frozen
B0 and the package candidate in separate fresh realms under canonical warmup
and ten exact create/action/abort warmups (including commit and rollback).
Keep all five forced-GC heap readings before and after each of three measured
lifetimes. Report every residual against the original 16 KiB budget, without
warming until a favorable result appears. In each realm also hold/release a
128 KiB numeric-array positive control: distinguish failure to resolve retained
objects from product/runtime residue. Keep control observations separate from
product observations. A first persistent post-drop failure gets one bounded
heap snapshot for retainer/category inspection; no repeated snapshot search.
This calibration cannot establish retained-heap acceptance or waive previous
failures. Preserve timeouts/partial results and stop rather than expanding it.

The first allocation correction makes duplicate/removal lookups conditional.
Its five-profile reproduction reduces the mean gap but leaves the 256-byte
decision inconclusive over observed ranges. Preserve that intermediate source
and report; do not call it passing. The remaining sampled map/bucket is also
unnecessary with one insertion. A second bounded correction stores that one
anchor directly and uses the already prepared incarnation, retaining bucket
lookup for multiple insertions. Repeat only the same fixed C02 reproduction;
no other cell, count, threshold or budget changes.

The first calibration's nominal 128 KiB positive control used 16,384 small
integers; Chromium's compressed representation made its backing storage about
64 KiB. Preserve that complete run as exploratory calibration, including its
baseline drift. Correct to 32,768 integers for the specified 128 KiB backing
store and repeat the fixed calibration once within the original three-minute
deadline. This is a control-fixture correction, not a relaxed byte gate or an
extension of calibration time. Preserve the original runner bytes separately.

### Host and tools

The same machine and tools as the full-flow lab. Background load is recorded
with each set.

## Conditions

### Stage 6: independent memory phase (2026-10-26 UTC)

Chromium **156.0.8078.4** used the installed Playwright host and a
manifest-derived import map resolving the actual experimental package entry.
No network content, dependency change, bundler or transpilation was used.
Node 24.12.0 coordinated the same macOS x64 Xeon/64 GiB host. Continuation
load averages rose from 7.23/14.89/17.30 to 100.39/58.09/36.79; profiled
phase durations on that busy host are not runtime latency evidence.

B0 remained the frozen measured collection hash
`b2afe023ca3a1ebd81b741e6726273c97ce03a860702496fa70c8db928606f76`;
B1 remained checkpoint hash
`7610e112f6a165f95bbcd13903c92d50e30853729499aca5a884926388405af1`.
Current regular-core fixes have their separate recorded hashes and did not
replace B0. The baseline's recorded public revision is
`02a70ed8d032b83b27e066f51071baca3a0a76b0`; the memory coordinator captured
source hashes rather than a fresh live HEAD. Do not infer a later commit from
the baseline revision. The experimental source did not change during memory.

The original memory coordinator stopped for the Q/C fixture-warming correction;
its exact old source and pre-stop report were preserved. Browser cleanup
completed before stopping its stale CDP wait. The continuation retained the
original deadline, reused 410 valid profiles and collected 390 missing profiles.
Earlier small fresh-action estimates remain exploratory. No counts or gates
changed. Each retained case/variant uses its own warmed realm, then five fresh
product lifetimes per transaction mode with all five GC observations per state.
Warmup exercised ten-item canonical collections, not every retention helper
or large-case branch; one-time probe/runtime/code footprint remains unisolated
in empty-baseline comparisons. Temporary inputs/oracles/references are dropped;
only the explicit caller-hold experiment intentionally keeps old keys.

### Stage 6: checkpoint investigation and admission (2026-10-26 UTC)

All three sets used Node 24.12.0 on macOS 15.6 x64, 16 logical CPUs on
the same Xeon host with 64 GiB memory. B0 loaded the byte-preserved measured
baseline; B1 resolved the actual `@canonic2/lattice/experimental/collection`
package export. Source verification ran before measurements. The ordinary
entry still targets the original source; the prototype was not edited.

Initial checkpoint hash
`c45e032bca979be04c6f285ea0a44a24e81883af902614693a6eeaa45ede0f7d`;
corrected checkpoint hash
`7610e112f6a165f95bbcd13903c92d50e30853729499aca5a884926388405af1`.
The reporter and measured baseline/prototype hashes remain those recorded
above. The manifest hash was
`fb72313f5a16fa52cac9ca79f8e015b26aa96ef221a9392a3772d4492b07e868`.
Both checkpoint versions and the manifest were snapshotted separately.

Each teardown reproduction used three fresh OS processes, both variants in
each process with alternating first position, two warmups and seven measured
single-append blocks. Setup, capture, validation and abort were separate
timers. Profiles and three-pair ordinary/pre-GC controls ran afterwards in
separate fresh processes, excluded from primary timings. Initial reproduction
load averages were 7.82/9.04/9.93 before and 17.49/13.53/11.63 after;
corrected reproduction 13.88/13.08/11.58 before and 10.36/14.04/12.63 after.
No timing outliers were excluded.

The corrected checkpoint admission used the same twenty-case driver and
five-warmup/fifteen-measured-block protocol, with separate semantic/counting
validation and uninstrumented action clocks. Q01/Q02 both used 1,000 matched
actions per block; cold samples used five/100 lifetimes respectively.
It recorded public revision `02a70ed8d032b83b27e066f51071baca3a0a76b0`,
load 10.13/13.04/12.34 before and 21.39/15.55/13.32 after. The admission
coordinator hash was
`07b65e34533b60078e78f43b40adc98e3bb0e2506e6fe0421666e1aba3e54990`;
the package composition adapter was
`8bfdb56880093552a74ba908ddea712894c7428ccfac46c2cbe6fc533c1f5d25`.
Chromium was not launched because the Node setup gate stopped admission.

### Stage 6: eight-minute diagnostic amendment (2026-10-26 UTC)

Node 24.12.0, macOS 15.6 x64, 16 logical CPUs on a 3.20 GHz Intel Xeon
W-2140B with 64 GiB memory. Power state was unavailable. Load averages were
60.02/23.87/15.53 before and 11.51/17.17/14.66 after; initial host contention
was substantial. No observer or profiler ran in the primary setup or bulk
measurements. The clock probe observed a minimum positive increment of
0.115 µs. Raw blocks and separate instrumented traces were saved locally.

Public revision `02a70ed8d032b83b27e066f51071baca3a0a76b0`; shipping
collection, reporter and candidate hashes exactly matched those recorded for
the reduced run. No measured implementation changed. The new fresh-process
endpoint hash was
`b500816add8c1ad4d7f9e664a674d99a9e0e2b00641b091da10c99977c4bb635`;
the coordinator hash was
`e7bcf2f5cc524059a92c63c76cd9d3a0dcb91d7b60def4c609d05aa8a0f07633`.

The owner clarified within-process interleaving before measurements. Three
fresh OS processes each ran both variants, strictly alternating the first
variant across paired blocks; process 2 started with B1, processes 1/3 with
B0. Each variant had 20 warmups then 200 measured blocks of 100 fresh
setup/removal/abort lifetimes. Setup timing excluded reference capture,
removal, abort, correctness and IPC; teardown timed abort separately.
No setup process was pooled with another. Instrumented attribution used two
additional fresh processes, 60 blocks of 100 lifetimes per variant; these
blocks are excluded from the primary results.

Bulk used a new fresh process per workload, interleaving both variants and
alternating which ran first. Each variant had two warmups then seven measured
single-action blocks. Fixture creation, reference capture, validation and
abort stayed outside action timing. API input conversion and commit cleanup/
delivery stayed inside. The helper's capture-contaminated lifetime span was
not used: only separately bounded setup, action and teardown were reported.
There was no calibration, forced GC, allocation sampling, matrix expansion,
Chromium run or independent confirmation.

### Stage 6: approved reduced run (2026-10-26 UTC)

The reduced run used Node 24.12.0 on macOS 15.6, x64, a 16-logical-CPU
3.20 GHz Intel Xeon W-2140B host with 64 GiB memory. Load averages were
9.60/7.20/7.73 before and 7.68/7.13/7.66 after; power state was unavailable.
No browser was launched because the Node gate failure stopped the phase.

The public revision was `02a70ed8d032b83b27e066f51071baca3a0a76b0`.
Measured shipping collection, reporter and local candidate hashes matched the
previous run exactly: collection
`b2afe023ca3a1ebd81b741e6726273c97ce03a860702496fa70c8db928606f76`,
reporter `e4ad2ac9d60e185067e23107b1133b5f5067dbc16c5b4b9dc41401ce3ad2104d`,
candidate `22c3d04f416610f613ad383cd7f7e9738458adbf3ab339d19d69cf293aaeaacf`.
The new shared comparison driver hash was
`c0a61ee6639a384c6b2dd0c6665b32f79462cd4728224e426b339401ca17d029`;
the Node realm dispatcher was
`e52502dbe9ff4af635b37af08b948ef6bde6c230696d48e7cba9b7f3e33eb292`;
the coordinator was
`9918737a2c8947ced4306754e0cec61aa6567a28ca43bc4cb6041c0e27b23763`.

Diagnostics and comparison used separate owned worker realms. Semantic stress
checks ran in a validation realm before each comparison block, never in its
timing realm. Subscriptions in acceptance timings, including reset/rebound
subscriptions, had no counters or metadata histories. Fixtures, reference
capture, exact correctness checks and resets were outside action timestamps.
Cold setup retained the original fresh input-generation/create/subscribe
boundary; post-action validation followed the contiguous lifetime timer.
Q01 had five cold lifetimes per block; Q02/Q03 had 100. Every logical action
started from the same canonical values, order and watcher roles in both variants.

Q01/Q02/Q03 used matched action counts of 1,000/2,000/4,000 respectively,
calibrated once to at least 20 ms within the approved caps. Every case had
five warmups and fifteen measured blocks per variant in seeded paired order.
Individual block durations remain in the raw report; calibration reaching
20 ms does not imply every later block reached 20 ms. Large-case independent
counts were approved but not exercised before the stop.

### Stage 6: resumed evaluation (2026-10-26 UTC)

| Item | Conditions |
| --- | --- |
| Amendment | Recorded before continuation: 100,000 actions only for cheap/single-change calibration; all other caps, live-item limit, performance gates and wall ceilings unchanged |
| Small calibration | Node 01:28:42–01:29:32 UTC; Chromium 01:30:19–01:31:33; all 80 variant cells per engine reached 20 ms |
| Extended calibration | 01:36:37–02:02:54 UTC; stopped and preserved as a runner correction, 139 paired cells, no main timing cells |
| Corrected initial set | 02:08:23–02:21:37 UTC; shared the original 01:36:37 start/deadline; watchdog enforced the original 45-minute ceiling without reset |
| Host and runtimes | Same Darwin 24.6.0/x86-64, Xeon W-2140B, 16 logical CPUs, 64 GiB; Node 24.12.0; calibration Chromium 156.0.8078.4, headless and cross-origin isolated |
| Background load | One-minute load about 4.0/5.1 at resumed Node/Chromium calibration starts; 21.4 at corrected initial-set start. Other workspace work was not suspended; no competing benchmark or suite was launched by this evaluation |
| Power | Unavailable: workspace policy rejected the read-only power-state command. No alternate command was used |
| Source | Public revision `ec72064dc3e82a1d2c0490623757b45a488697e7`; control/reporter/candidate hashes unchanged from the earlier smoke table |
| Corrected driver hashes | Shared harness `ee4108ee2d81970230e700180efdb7b8eddb8e8470872f37c7709736d57c283d`; measurement helpers `664132a7cca64b815cfcaef12c2ce3547aef9b420506108e69e90ced658ba491`; warmed driver `35a37446c8fb09f8fed011d892274a1d76b8c430eb1ea9271096fcee64dd1bc9`; Node runner `b8258619667a909dba1b00ba328994948e7a850f63fcdfa0b4888a9d16e7bb99` |
| Main sampling | Seed-1 matrix shuffle, paired seeded variant order; same fixed steady-state repetition count for both variants of each cell; five warmups, fifteen measured blocks |
| Cold sampling | Separate fresh create/action/abort samples: 100 lifetimes/block at N <= 100, five at N = 1,000, one at N = 10,000; construction and subscription setup reported separately; validation outside timing |

Both interrupted Node processes ended; completed raw reports remain local.
Chromium calibration closed its browser context/profile process. No external
traffic, downloads or dependency changes were needed. The current driver uses
two warmed collections per cell, at most 40,000 live items for the largest
input/output shape, with reset and validation outside action timing. Fresh
oracle and lifecycle cases are separate; their costs are not excluded from
the reported cold metrics. The prototype stayed unchanged throughout.

### Stage 6: smoke and calibration (2026-10-26 UTC)

| Item | Conditions |
| --- | --- |
| Approval | Owner approved prototype evaluation on 2026-10-25 with the mixed-action amendment recorded before runs |
| Host | Darwin 24.6.0, x86-64; Intel Xeon W-2140B, 16 logical CPUs, 64 GiB RAM |
| Tools | Node 24.12.0; installed Playwright Chromium 156.0.8078.4, headless, cross-origin isolated page |
| Measured source | Public revision `ec72064dc3e82a1d2c0490623757b45a488697e7`, plus uncommitted source hashes below; source files served/imported unchanged |
| Control SHA-256 | Collection `b2afe023ca3a1ebd81b741e6726273c97ce03a860702496fa70c8db928606f76`; shared reporter `e4ad2ac9d60e185067e23107b1133b5f5067dbc16c5b4b9dc41401ce3ad2104d` |
| Candidate SHA-256 | `22c3d04f416610f613ad383cd7f7e9738458adbf3ab339d19d69cf293aaeaacf` |
| Harness SHA-256 | Shared fixtures/probes `d8286836022ab5535fd20012e79eaac4c8f8d8d805336a32012f21f8488be3e4`; Node runner `56f8c4af2b5c2c8aaf7eb288e7e3d8e65d91a08d2351101b4dac0befbe5aeadf`; browser runner `940f0d87379a1249933d304f05c6b348ba1dee589985a29326efb7377beed006` |
| Run windows | Node smoke 01:15:23–01:15:44 UTC; Chromium smoke 01:16:34–01:16:53; Node calibration 01:17:03–01:17:04 |
| Background load | One-minute load approximately 5.1–5.9 in smoke, 4.9 in calibration; other workspace activity was not suspended. No other suite was launched by this lab |
| State and ownership | Fresh collection fixtures per action, ordinary warm process/module caches; owned browser profile/context, no actual network traffic, no downloads or dependency changes; browser closed after smoke |
| Calibration | 1,000-action priming block, then one measured block of at most 10,000 actions, in chunks bounded by 100,000 prepared items; no profiler or forced GC during timing |

Power state was not collected before the early calibration stop. No full
timing or profiling set is described as host-qualified acceptance. The control
and reporter hashes were checked again after these runs and were unchanged.

### Initial comparison (2026-10-25)

| Item            | Value                                                                                                                                                                         |
| --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Dates           | 2026-10-25: timing 00:34–00:37 UTC, counted 00:37, memory 00:37–00:40                                                                                                         |
| Host            | macOS 15, x86-64, Intel Xeon W-2140B with 16 threads, 64 GB memory; Node 24.12.0; Chromium 156.0.8078.4 bundled with Playwright 1.64.1, headless, cross-origin isolated pages |
| Background load | 1-minute load average 27 falling to 11 during the timing set, 11 to 12 for the others: the quietest runs of these labs                                                        |
| Samples         | Timing: 15 pages per variant and size; memory and allocations: 10; counted: 3. No page errored                                                                                |
| Measured source | The store, element helpers and example element as committed with the store's write change, plus the lab's capability, bindings, reconciler and runner, hashed in each report  |

### Stage 3 (2026-10-25)

| Item            | Value                                                                                                                                                                            |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Dates           | Timing 01:16–01:24 UTC, counted 01:24–01:25, memory 01:25–01:33                                                                                                                  |
| Host and tools  | As stage 1                                                                                                                                                                       |
| Background load | 1-minute load average 7 to 9 during timing and counted; memory began at 9 and ended at 45, which affects heap and allocation figures less than timings                           |
| Samples         | Timing: 15 pages per variant and size; memory and allocations: 10; counted: 3. No page errored                                                                                   |
| Measured source | Lattice as committed with bulk compaction, unchanged in both variants, plus the lab's two capabilities, bindings, summary bindings, reconciler and runner, hashed in each report |

### Stage 3 follow-up (2026-10-25)

| Item            | Value                                                                                                                             |
| --------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| Dates           | Timing 01:41–01:43 UTC, counted 01:43, memory 01:43–01:45                                                                         |
| Host and tools  | As stage 1                                                                                                                        |
| Background load | 1-minute load average 49 falling to 28 during timing, the busiest of these runs; 19 to 25 for counted and memory                  |
| Samples         | Timing: 15 pages per variant and size; memory: 10 per variant and size, for variants 1, 1n, 3 and 3n; counted: 3. No page errored |
| Measured source | As stage 3, with `setDoneMany` in both capabilities and the summary binding switchable for the diagnostic variants                |

### Stage 4 (2026-10-25)

| Item            | Value                                                                                                                                                                                              |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Dates           | Timing 02:08–02:21 UTC, counted 02:21, memory 02:22–02:32                                                                                                                                          |
| Host and tools  | As stage 1; the prototype tests ran with the package's Vitest in Node 24.12.0                                                                                                                      |
| Background load | 1-minute load average 9 to 21 during timing and counted; memory began at 15 and ended at 26                                                                                                        |
| Samples         | Timing: 15 pages per variant and size; memory and allocations: 10; counted: 3. No page errored                                                                                                     |
| Measured source | Lattice as committed, unchanged; the lab's collection prototype after the pre-run amendment, its tasks capability and bindings, the stage 3 variants, reconciler and runner, hashed in each report |

### Stage 5 (2026-10-25)

| Item            | Value                                                                                                                                                                                                                                          |
| --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Dates           | Timing 02:50–03:06 UTC, confirmation 03:06–03:16, counted 03:16–03:17, memory 03:17–03:27, garbage collection 03:27–03:35                                                                                                                      |
| Host and tools  | As stage 1; Chromium's trace categories `devtools.timeline`, `v8` and `disabled-by-default-v8.gc` for the collection events                                                                                                                    |
| Background load | 1-minute load average 6 to 9 during the timing set; 9 rising to 67 by the end of the confirmation; 70 to 88 at the start of counted and memory, falling to 13; 12 to 13 during garbage collection                                              |
| Samples         | Timing and confirmation: 15 pages per variant and size; memory and garbage collection: 10; counted: 3. No page errored                                                                                                                         |
| Measured source | The shipping collection as implemented, served unchanged from the package source (`collection.js` SHA-256 `a597d35a…`, `report.js` `e4ad2ac9…`, `store.js` `9bf3b436…`), with the lab's variants, reconciler and runner, hashed in each report |

## Results

### Stage 6: single-append correction and retention calibration (2026-10-26 UTC)

**Outcome: attributable append overhead corrected; retention measurement is
not ready for acceptance.** These are bounded diagnostic reproductions, not
confirmation or a substitute for the unresolved setup and retention gates.

The original measured package checkpoint is preserved at public commit
`a92011d64fe6c979c9b6450b101d15daec8ac856`, source SHA-256
`7610e112f6a165f95bbcd13903c92d50e30853729499aca5a884926388405af1`.
The correction is commit `dfc342e66eccb33a213fa778541ec5ca304c051f`, source
`406e39c85a96d6b9b341fc62647c3c33e41324e60d00f8a0f52cc9dbd0d0b116`.
The intermediate conditional-set source, hash
`6aaeeed3671aba587fb20e0b0ad530a6ac219040e816e24d9b35bff49c208bf8`,
is separately preserved locally with its report. Frozen measured B0 and all
earlier reports remain intact. The independent regular cleanup commit is
`193c1620599e730bdecce22edee12839ea47a9df`; regression/consumer coverage is
`f1fa16a038dd782e21656cc34610ae683cde3f3c`; preceding findings are saved in
`154c3b4e1f16634c858ee7c419363d6ac93bf308`. Pending regular-API changes and
unrelated work were excluded from these commits.

#### Single append

Chromium 156.0.8078.4 used the actual experimental export and unchanged frozen
B0, with five 1,000-action profiles per variant per reproduction, alternating
first variant. Preparation/warmup/reset, validation and abort stayed outside
512-byte sampling; collected allocations were included. Every bounded action
chunk was below the 100,000-live-item cap. All executed result/identity checks
completed; no page or cleanup errors occurred. Runs took 3.108, 2.370 and
2.144 seconds respectively. Source bytes, profiles and reports are preserved.
Per-reproduction source hashes identify uncommitted measured bytes alongside
the recorded repository HEAD; the final commit preserves those same bytes.

Values below are estimated bytes/action, with n=5 independent profiles per
variant. At five profiles p95 equals maximum. The gap envelope is candidate
minimum minus control maximum through candidate maximum minus control minimum;
it is descriptive, not a confidence interval.

| Revision | B0 mean; p50 / p95 | B1 mean; p50 / p95 | Mean extra | Gap envelope | 256-byte allowance |
| --- | --- | --- | --- | --- | --- |
| Measured checkpoint | 1,193.422; 1,189.360 / 1,217.452 | 1,615.622; 1,621.904 / 1,647.620 | 422.200 | 353.872–469.552 | Fails diagnostic range |
| Conditional sets | 1,201.103; 1,192.984 / 1,226.964 | 1,459.050; 1,466.840 / 1,481.720 | 257.947 | 186.408–300.920 | Inconclusive |
| Single insertion without buckets | 1,209.004; 1,207.352 / 1,222.848 | 1,276.607; 1,272.348 / 1,297.536 | 67.603 | 32.448–96.084 | Fits diagnostic range |

Total allocation samples B0/B1 are 10,999/14,997, 11,074/13,548 and
11,139/11,876. The original 375.871-byte failure remains its original evidence;
the new before run reproduces the same direction with a larger control gap.
No original sample was replaced or pooled with these diagnoses.

The stacks and source identify unnecessary general-plan bookkeeping: a
removal set without removals, an insertion-duplicate set with only one
insertion, and a map plus bucket for that insertion's one anchor. The first
correction removes unnecessary sets but does not establish the gate. The
second stores the one anchor directly and uses the already prepared
incarnation. Multiple insertions retain duplicate detection and ordered
buckets; removals retain membership lookup. Both follow the same validation,
write guard, order construction, undo log and delivery. No global caching,
retained index or change to ordinary operations was introduced. Sampled sites
are attribution evidence, not exact individual object sizes.

Report SHA-256, before/intermediate/final:
`f9f6b4c0f4b82f70686926a10c3401845181c8c927524b14748e47e001273d8c`,
`6a5e8a35c654aa0da9890ec882b6735277da2d299c0f00acd0d329fd0ca596b4`,
`209546518dbcc4343aa01cb978777dd841995a49c3671bd4788d54ec862d67b3`.

#### Separate retention calibration

The exploratory control-size run took 7.377 seconds; its corrected repetition
took 6.933 seconds. Including diagnosis/correction, both finished within the
original 16:20:40.201–16:23:40.201 UTC deadline. Both used the final experimental
source above and frozen B0. Eight fresh realms per run cover both variants,
two fixed shapes and two fixed warmup conditions; each has three independent
product lifetimes (commit, rollback, commit), not independent OS processes.
All five GC readings per state remain saved; the table gives each lifetime's
post-drop residual in bytes against its own preceding empty baseline.

| Warmup | Shape | B0 residuals | B1 residuals |
| --- | --- | --- | --- |
| Canonical only | Empty, N=10 | 20,188; 1,132; 6,232 | 20,576; 1,132; 6,232 |
| Canonical only | Full replacement, N=1,000 | 111,660; 9,524; 21,716 | 110,200; 8,196; 22,820 |
| Ten exact lifetimes | Empty, N=10 | 5,372; 188; 16 | 5,200; 188; 16 |
| Ten exact lifetimes | Full replacement, N=1,000 | −19,532; 7,196; −6,320 | 912; 7,112; −5,840 |

Canonical warmup reproducibly leaves a shared first-use footprint exceeding
16 KiB. Exact workload/probe warmup reduces it, supporting a warmup mismatch
in the earlier measurement. It does not settle the instrument: in exact-warmed
B0 replacement, the held 128 KiB control registers only 99,148 bytes and its
release ends 31,948 bytes below the starting baseline. Seven other realms
register 131,136–131,168 bytes held and 60–72 bytes after release. That one
approximately 32 KiB baseline shift exceeds the retention budget and cannot
be dismissed or pooled away. The earlier 64 KiB exploratory run shows a
similar shift (−32,240 bytes), so this concern survives the fixture correction.
No extra warmups, samples or calibration search followed.

The fixed first-failure snapshot is from B0's canonical empty action, after
its first product lifetime. It contains no named per-instance `watchItem`,
`watchKeys`, `commitBatch`, `rollback`, `subscribe`, `deactivate`, `release` or
`updateMembership` closures; exported factory helpers remain. This supports
release of those instance contexts at that point. The heap includes native,
code, shape and shared closure storage; without a matched before snapshot
its categories cannot attribute the 20 KiB delta. Snapshot collection may
perturb that realm's later two lifetimes; other realms do not capture a heap.
Neither this inspection nor negative deltas prove leak freedom.

The corrected report hash is
`44bd97c6acc573ef3aa835f26ba23cb44d2412081887b65d553ebe08fd0d85d0`;
the preserved exploratory report is
`0f59fdd2f8026ea691a60ba8285d37a17a33c08c5c2a1679bff7ab2aa6cdbe43`.
All owned browsers closed. No substantial retention run, setup rerun or
adoption confirmation was launched.

### Stage 6: independent allocation and retention outcome (2026-10-26 UTC)

**Outcome: large membership allocation targets fit; one small-call allocation
gate fails; retained-heap acceptance remains unestablished. Adoption stays
blocked.** The independent phase completed in **34m 33.588s**, including the
runner correction and pause, within the original combined sixty-minute
deadline. All twenty cases completed: 800 complete allocation profiles (400
actual and 400 matched controls), 400 retained samples, twenty caller-held-key
observations, commit/rollback weak-reference probes, and four named 100-cycle
churn sets. Confirmation was not run.

The continuation reused 410 valid complete profiles and collected 390 missing
profiles. Twelve completed fresh small-action profiles remain exploratory,
not warmed-condition acceptance; one interrupted profile remains incomplete.
The original report, pre-correction source bytes and every artifact remain
preserved. The experimental implementation did not change during this phase.

#### Allocations

Values are arithmetic mean **estimated allocated bytes per action** across
five independent profiles, not exact allocation totals. Action timing includes
API input conversion and delivery; setup/reset/validation/counters are outside
sampling. Complete-lifetime profiles include input generation, creation,
action and abort. Matched controls are separate; no favorable control was
subtracted from the values below. Large profiles contain one action; Q/C
profiles contain 1,000 in bounded chunks. Sampling interval stayed 512 bytes,
including minor-/major-collected objects.

| Case | Action mean B0 / B1 (bytes) | B1 / B0 | Lifetime mean B0 / B1 (bytes) |
| --- | --- | --- | --- |
| B01 | 15,444,961.6 / 430,909.6 | 2.79% | 16,265,315.2 / 1,281,331.2 |
| B02 | 1,504,797,301.6 / 4,661,298.4 | 0.31% | 1,511,730,284 / 11,519,525.6 |
| B03 | 7,050,569.6 / 272,720.8 | 3.868% | 7,693,279.2 / 896,860.8 |
| B04 | 708,719,379.2 / 2,625,309.6 | 0.37% | 715,432,289.6 / 9,339,866.4 |
| B05 | 12,450,267.2 / 715,676 | 5.748% | 13,084,736.8 / 1,343,952.8 |
| B06 | 1,213,737,101.6 / 6,680,596.8 | 0.55% | 1,220,584,213.6 / 13,356,872.8 |
| B07 | 7,033,413.6 / 248,760 | 3.537% | 7,295,776 / 503,726.4 |
| B08 | 118,730,647.2 / 371,386.4 | 0.313% | 125,370,185.6 / 7,094,300 |
| B09 | 214,857,043.2 / 1,497,123.2 | 0.697% | 221,693,963.2 / 8,221,864.8 |
| B10 | 1,214,169,855.2 / 7,044,036 | 0.58% | 1,220,927,187.2 / 13,762,152 |
| Q01 | 145,692.91 / 16,616.217 | 11.405% | 373,879.397 / 244,856.866 |
| Q02 | 7,698.898 / 7,178.386 | 93.239% | 79,170.665 / 79,735.398 |
| Q03 | 2,013.435 / 2,338.147 | 116.127% | 72,269.786 / 72,742.432 |
| C01 | 259.635 / 387.354 | 149.191% | 11,617.263 / 11,738.18 |
| C02 | 1,239.793 / 1,615.664 | 130.317% | 13,054.543 / 13,480.648 |
| C03 | 1,621.75 / 1,771.77 | 109.25% | 13,422.852 / 13,769.928 |
| C04 | 263.859 / 266.924 | 101.162% | 11,631.919 / 11,635.298 |
| C05 | 427.766 / 400.057 | 93.522% | 12,250.962 / 12,264.341 |
| C06 | 243.368 / 206.798 | 84.973% | 12,126.44 / 12,110.896 |
| C07 | 153.209 / 110.628 | 72.207% | 11,955.661 / 11,985.73 |

The companion compact summary records n, p50, p95=max, observed range, IQR
and sample counts for every cell/scope. Observed profile ranges diagnose
uncertainty; they are not formal confidence intervals. All eight selected
membership-only large cases fit the 35% action-allocation limit over their
observed ranges, with mean reductions **94.25–99.69%**. All ten large-case
lifetime comparisons fit the unchanged 10% non-regression allowance. Mixed
savings are reported without imposing the membership-only reduction gate.

For 10,000 items, estimated action allocations fall from 1,504.797 MB to
4.661 MB for append, 708.719 MB to 2.625 MB for removal, and 1,213.737 MB
to 6.681 MB for replacement (decimal MB). Their p50/p95=max B0/B1 are:
append 1,504,800,284/1,504,818,596 versus 4,666,504/4,680,720 bytes;
removal 708,705,088/708,806,148 versus 2,630,744/2,645,252 bytes;
replacement 1,213,611,892/1,214,137,452 versus 6,548,220/6,984,472 bytes.
Total sample counts across five action profiles are 116,026/15,967,
483,647/5,471 and 598,406/21,902 respectively.

**C02, one bulk append at ten items, fails the small allocation gate.** Mean
additional allocation is **375.871 bytes/action**, against 256 allowed.
The observed profile-range difference is 345.872–414.220 bytes, entirely above
the limit. Baseline p50/p95=max is 1,242.104/1,246.656 bytes; candidate
1,612.432/1,638.848. Candidate sampled sites include about 576 bytes/action
at `updateMembership`, 266 at `Set`, 96 at `Map` and 281 at outer-batch
commit bookkeeping. These site estimates identify plan/order bookkeeping
costs, not precise object sizes. The ordinary single-insert control is
separate; do not hide this failed bulk call behind a different cell.

Q03, one removal at 100 items, has mean additional allocation 324.712 bytes,
but its observed difference range, 218.612–394.188, crosses 256:
**inconclusive at the approved sampling scope**, not passing. C01 empty bulk
and C03 two prepends have mean increases 127.718 and 150.020 bytes; observed
ranges fit 256 (C03 upper difference 250.512). That narrow observed margin
is not independent confirmation. No extra profiles or candidate tuning were
added to obtain a favorable result.

#### Retention and release

Every case/variant has five collection lifetimes each for commit and rollback,
with empty, initialized, live-after-action and dropped snapshots. All five
forced-GC observations at every state are preserved. A fresh realm is warmed
per case/variant, then reused for five independent product lifetimes; these
are not five independent fresh processes. Inputs, oracles and reference
captures do not remain in retained snapshots; caller-held keys are separate.

Of forty case/transaction cohorts, live-overhead observed ranges fit in 34,
are inconclusive in three (B07 commit/rollback and Q01 rollback), and contain
numerical failures in three: B03 rollback (47,920 bytes candidate excess in
one replicate), B04 commit (24,976/27,096 bytes in two replicates), and B05
commit (24,964/37,244 bytes in two). Failed replicates are not pooled away.

The post-drop **16 KiB gate is not established**. Both variants exceed it
in at least one replicate of all twenty commit cohorts and two rollback
cohorts (B02/B09); the other eighteen rollback cohorts fit. Even empty-bulk
and existing empty-batch controls show first-sample residuals around 20 KiB
in both variants. Large first-sample residuals are roughly 100–132 KiB.
Later samples near their own empty baselines do not erase those failures.
Cold probe/runtime/code initialization has not been separated from product
retention. This is an attribution/calibration gap, not proof of a
candidate-specific object leak and not an acceptance pass.

All commit/rollback weak-reference observations were false for the targeted
callback captures, selected values, discarded records and old keys in both
variants. This supports release of those objects, not the retained-byte gate.
Explicit caller-held old arrays were observed through disposal then released;
their post-release heaps also include the shared residual footprint.

All four churn sets completed exactly one hundred cycles. Values are bytes
above the original empty baseline, retaining every observation:

| Case / variant | After cycle 1 | Final | Cycle 1 to final increase | Largest cycle observation |
| --- | --- | --- | --- | --- |
| B06 / B0 | 115,556 | 156,148 | 40,592 | 1,734,328 |
| B06 / B1 | 112,756 | 155,852 | 43,096 | 1,711,292 |
| Q02 / B0 | 38,072 | 75,828 | 37,756 | 101,240 |
| Q02 / B1 | 37,848 | 68,548 | 30,700 | 130,604 |

These sets do not meet the no-growth/16 KiB rule. Growth occurs in steps and
includes transient high observations: B06/B1 reached 1,711,292 bytes at cycle
75 and returned to 155,852 at cycle 100. Late Q02 values plateau, but its
initial-to-final growth still exceeds budget. Do not discard spikes, redefine
the empty baseline after observing growth, or claim leak-free acceptance.
A bounded calibration/retainer investigation is needed before repeating a
large retained-heap acceptance set.

The preserved partial report hash is
`bb6880b743891ecba5bff9a51ff194c0f9167ea738f9055857270e0575321321`;
the final report hash is
`4d1fa1781d1f6cc177a57e609127ed47a6b246b563580b8e8eba673ba53dc1b2`. Aggregation reads saved data only and keeps
input snapshots. It combines only completed profiles with valid conditions;
it changes no threshold and authorizes no further runtime measurement.

### Stage 6: setup-only profiles and independent regular-core cleanup (2026-10-26 UTC)

The regular collection contained the same redundant normal-abort listener
removal as the experimental checkpoint. Its abort callback now clears the
already-ended registration before releasing the subscription; committed
removal and failure with a live owner still explicitly detach. The store's
normal abort already avoids that redundant call. Its initial-callback catch
nevertheless removed the listener again when the callback aborted and then
threw; it now detaches only while the record is still active. Original errors,
ended callbacks, and live-signal failure cleanup remain unchanged. These are
independent fixes to accepted cores, with no bulk API adoption or dependency
on the experimental module. The measured frozen baseline remains byte-identical.

Current regular collection hash:
`1cbd0b38c8a9274c00c5a8f2692319ce84886a659ff69eef5786510fd5b96907`;
store hash `368915a3f34abd4cfc2ed4f709473f4529019dbf53e99537a09478a1024ddd5e`.
The new work-count/lifecycle regression cases belong with their respective
core tests; this record makes no new standalone core timing claim.

Setup-only profiling completed in **16.699 seconds**, within its three-minute
ceiling, using three fresh Node processes and the actual experimental package
entry against frozen B0. Each CPU/allocation profile contained 1,000 subscribed
100-item setups, with validation and abort after the profiler stopped. CPU and
allocation profiling were separate; no mixed action/reset ran in either window.
Each process alternated first variant between CPU and allocation modes. The
maximum prepared product state was 100,000 live items. These are diagnostic
profiles, not another timing admission set or an individual setup-latency test.

Source inspection finds identical initial record conversion, item-map/order
construction and watcher registration algorithms. The checkpoint adds one
per-collection bulk-method closure and a returned method property. Its bulk
validation/order-building body is not executed during setup. The corrected
abort callback runs after the profiled setup, not inside it.

| Fresh process | First CPU variant | CPU-profiled setup total B0 / B1 (ms) | Sampled allocation B0 / B1 (bytes/setup) |
| --- | --- | --- | --- |
| 1 | B0 | 389.209 / 209.809 | 167,378.016 / 167,881.048 |
| 2 | B1 | 216.272 / 315.207 | 167,835.944 / 167,463.808 |
| 3 | B0 | 305.539 / 189.466 | 167,143.264 / 167,293.952 |

Sampled setup allocation differences are +0.30%, −0.22% and +0.09%, changing
sign and lying within observed sampling variation. The first CPU-profiled
variant is slower in every process. That identifies a strong profile-order/
history effect in these totals, not a candidate-specific recurring CPU penalty.
Both profiles show record construction, item watchers, native event-listener
registration and GC. GC accounts for roughly 20–28% of CPU samples in both
variants; no unique heavy candidate setup path was isolated. The largest
allocation sites are `watchItem` (about 65 MB per 1,000 setups), collection
creation (about 37 MB), shared selector/caller work and native listener setup,
with comparable distributions across variants.

The GC event observer delivered no entries before these profile records were
captured, despite the CPU profiles containing GC samples. Its empty event
arrays therefore cannot prove the absence of GC or support precise event/time
correlation. Keep that instrumentation limitation, the profile perturbation,
shared warmup history, and holding a batch of fixtures explicit. These profiles
do **not** identify the cause of the previous admission setup-p95 tail, and do
not clear or discard it. No speculative setup optimization was applied.

Source hashes and all raw CPU/allocation profiles are preserved. The saved
setup-profile report hash is
`2e66b71e619959e43ba7e97411a0a6652f753c4a6838adad4f8fd5b4b7d036f3`.
Memory evidence proceeds independently under the owner's explicit amendment;
adoption remains blocked and confirmation does not follow from these profiles.

### Stage 6: checkpoint cleanup correction and admission stop (2026-10-26 UTC)

**Outcome: attributable cleanup inefficiency fixed in the experimental
checkpoint; acceptance still fails the setup-p95 gate.** No shipping behavior
was changed. The initial reproduction took 234.597 seconds and corrected
reproduction 212.534 seconds, each within its per-revision eight-minute ceiling.
Admission stopped after 72.702 seconds, preserving two complete Node cases.
There was no runner correction or deadline reset, and no cleanup error.

#### Reproduced append abort, before correction

Times are milliseconds per abort over seven measured blocks per process;
p95 equals max. No GC/profiler or counters ran in the primary blocks.

| Process | Baseline abort p50 / p95=max / IQR | Initial checkpoint abort p50 / p95=max / IQR | p50/p95 gate |
| --- | --- | --- | --- |
| 1 | 278.164 / 299.541 / 18.707 | 627.171 / 664.591 / 356.667 | Both fail |
| 2 | 268.516 / 355.050 / 9.170 | 679.649 / 786.110 / 464.878 | Both fail |
| 3 | 266.461 / 287.542 / 25.301 | 621.485 / 653.025 / 350.589 | Both fail |

The regression repeats through the actual experimental export, rather than
being confined to the old lab-copy import. The separately instrumented abort
measured 269.337 ms in B0 and 668.870 ms in B1. Native
`removeEventListener` accounted for 218/234 (93.2%) and 564/577 (97.7%)
CPU samples respectively; no GC event was observed during either abort.
Ordinary/pre-GC diagnostic medians were B0 318.272/277.727 ms and
B1 668.081/704.711 ms, three samples each. Pre-abort GC therefore did not
remove the checkpoint excess; its own cost is excluded from abort timing.

Source and the work-count reproduction identify a concrete redundant step:
abort callbacks are registered with `{ once: true }`, so platform dispatch
has removed the current listener before calling it. `release` then calls
`removeEventListener` for that already-removed registration, repeating native
listener-list searches. This inefficiency is present in both cores. The
profiles establish its dominant cost, not every runtime/heap detail behind
the difference in scan costs between the two variants.

The experimental callback now clears its `signal` and `onAbort` references
before deactivation. Committed removal, failed initial delivery and other
cleanup paths still explicitly remove live listeners. Ownership, idempotence,
per-subscription abort behavior and delivery registration order are preserved;
no signal pooling or automatic strategy switching was introduced.

#### Same comparison after checkpoint-only correction

| Process | Baseline abort p50 / p95=max / IQR | Corrected checkpoint abort p50 / p95=max / IQR | p50/p95 gate |
| --- | --- | --- | --- |
| 1 | 293.135 / 362.726 / 97.181 | 3.434 / 4.391 / 1.282 | Both fit |
| 2 | 263.105 / 293.156 / 14.973 | 4.261 / 7.508 / 3.758 | Both fit |
| 3 | 276.542 / 292.493 / 33.218 | 3.646 / 5.260 / 1.417 | Both fit |

All three independent processes fit the unchanged abort allowances. Corrected
checkpoint append-action medians remained 24.579/21.601/25.512 ms versus
baseline 4,337.914/4,221.333/3,784.845 ms. Those action figures are diagnostic,
not the full acceptance matrix. In the corrected abort profile B0 still
spent 216/227 samples in native listener removal; B1 had nine total samples,
none attributed to that function, with a 6.948 ms instrumented abort. The
short candidate profile cannot support detailed cost percentages. Ordinary/
pre-GC candidate abort medians were 4.835/4.871 ms. This controlled correction
and repeated comparison support removal of the redundant native searches as
the cause of the improvement. They do not establish allocation or retention.

#### Corrected checkpoint admission

The fifteen-block summaries below are **per-action/per-setup block averages**
in µs, not individual setup latency. P95 equals max at this sample count.

| Case | Action p50 B0 / B1 | Action p95=max B0 / B1 | Setup p50 B0 / B1 | Setup p95=max B0 / B1 | Setup IQR B0 / B1 |
| --- | --- | --- | --- | --- | --- |
| Q01: 1,000 items, ten removals, no watchers | 913.095 / 55.545 | 1,438.442 / 79.130 | 492.241 / 404.071 | 2,604.046 / 1,382.973 | 620.897 / 299.127 |
| Q02: 100 items, one mixed change, subscribed | 22.238 / 18.577 | 29.786 / 28.805 | 101.743 / 102.330 | 157.339 / 236.902 | 17.308 / 31.166 |

Q01 had no timing-gate failures. Q02 action and setup median allowances fit,
but setup p95 increased **79.563 µs** against **15.734 µs** allowed, an excess
of 63.829 µs beyond the allowance and larger than either IQR. The candidate's
worst setup block was round 11; the baseline's was round 6. Each cold block
averaged 100 lifetimes. Q02 teardown p95 also exceeded its gate: 145.378 µs
versus 128.342 µs, an increase of 17.036 µs against 12.834 µs allowed. That
4.202 µs excess is below either teardown IQR (10.907/9.634 µs); report it
as unresolved rather than an independently clear regression.

The driver stopped on the setup gate under the agreed rule against spending
memory/confirmation budgets on a clearly failed admission set. Its exploratory
fifteen-block p95 is sensitive to a single block. Host contention and previous
inconsistent setup findings remain threats, not reasons to discard the failed
block or call the gate passing. No setup-specific attributable correction has
been established by the abort investigation.

Only two of twenty Node cases completed; the remaining eighteen and every
Chromium case are unmeasured in this admission set. Allocation, retained heap,
churn and independent confirmation were **not started**, so those requested
phases remain incomplete. The prepared memory probe is unexecuted and supplies
no evidence. No numerical gate or sampling scope was relaxed to progress.

Read-only aggregation verified the unchanged saved reports, hashes:
initial reproduction `06f4f78739b7156d55f13dd1d45eeb6d199658a13330546b1e69f51dab3c9000`,
corrected reproduction `8e5a2bba4efe7163fc9f933c3e5b4e66870955bea4ec5137fcc83f4c7a0f3201`,
admission `2efe7311cce9214c80151ee8a32e201ba4bc683d340c7bbd89e8d84e7994aa73`.

### Stage 6: setup-tail reproducibility and intended bulk benefit (2026-10-26 UTC)

**Outcome: setup-tail difference inconsistent/unresolved; substantial intended
bulk action benefit; prototype unadopted.** Both phases completed in
245.385 seconds: 35.362 seconds for readiness/setup/attribution and
210.023 seconds for bulk, within the separate three-/five-minute ceilings.
No extra work was accumulated for timing resolution. All selected blocks
completed, with no cleanup errors or child-process stderr.

#### Setup, three independent process results

Values are µs per setup, summarized from **200 block-average setup times**
per variant per process, each averaging 100 setups. P95 is the 190th ordered
block average, not individual setup latency. Maxima and IQRs remain visible;
no tail block was excluded.

| Process | Setup p50 B0 / B1 | Setup p95 B0 / B1 | Setup max B0 / B1 | Setup IQR B0 / B1 | p95 increase / allowance | Gate |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 98.672 / 98.506 | 162.815 / 214.517 | 277.006 / 384.833 | 9.334 / 10.440 | 51.702 / 16.282 | Fails |
| 2 | 99.157 / 99.523 | 132.980 / 139.380 | 524.736 / 487.997 | 8.900 / 8.156 | 6.400 / 13.298 | Fits |
| 3 | 95.959 / 96.807 | 119.819 / 121.073 | 204.558 / 1,422.233 | 10.593 / 8.821 | 1.254 / 11.982 | Fits |

All three setup p50 allowances hold. Setup p95 fails in one process, so the
precommitted diagnostic label is **inconsistent/unresolved**, neither repeated
in at least two processes nor absent in all three. In particular, process 3's
large candidate maximum is retained even though its p95 fits the allowance.
The original reduced-run failure remains a failed observation.

The paired moving-block bootstrap (ten consecutive paired blocks, seed 1,
2,000 replicates) gives these 95% intervals for the candidate/control setup
p95 ratio: process 1, **0.650–1.896**; process 2, **0.627–1.391**;
process 3, **0.865–1.109**. Corresponding intervals for excess beyond the
unchanged p95 allowance are −99.462 to +91.897 µs, −102.389 to +35.425 µs,
and −31.347 to +0.979 µs. All span zero. They describe uncertainty, not a
waiver of process 1's numerical failure or a reason to add samples.

| Process | Construction p95 B0 / B1 | Subscription p95 B0 / B1 | Teardown p50 B0 / B1 | Teardown p95 B0 / B1 |
| --- | --- | --- | --- | --- |
| 1 | 68.389 / 92.879 | 99.585 / 95.852 | 41.438 / 41.125 | 54.323 / 52.374 |
| 2 | 57.810 / 57.990 | 74.739 / 83.239 | 41.382 / 41.647 | 56.631 / 59.873 |
| 3 | 48.991 / 47.000 | 72.578 / 71.530 | 42.060 / 42.358 | 58.887 / 59.673 |

All setup-case teardown p50/p95 allowances hold in this set. The elevated
component changes across processes; component p95 values are not additive.
The construction component retains the original fixture boundary, including
input generation and owner/controller creation; it is not pure factory time.
There is no repeatable isolated construction or subscription penalty here.

#### Separate attribution evidence

The instrumented passes observed 42 GC events in each variant, totaling
169.452 ms in B0 and 200.236 ms in B1; the longest events were 20.514 and
34.388 ms. Approximate timestamp overlap with setup was 74.360/159.024 ms,
and with abort 5.412/0.482 ms. These are diagnostic time correlations, not
allocation totals or proof that a candidate change caused a collection.
CPU sampling shows GC, event-listener registration, collection construction,
abort cleanup and record conversion in both variants; there is no isolated
candidate-only setup hot path established by these samples.

GC pauses can overlap setup in both implementations, but the instrumented
passes have different heap histories and do not identify the cause of the
untraced original tail. Combined with inconsistent primary results and high
initial host load, attribution remains **unresolved**. Do not relabel a
setup gate failure as a proven GC or host artifact.

#### Intended large operations: diagnostic action measurements

N = C = 10,000, representative subscriptions, exact original workloads.
Times below are milliseconds per complete logical action, including API
input conversion and synchronous commit delivery. Each summary has seven
measured actions; p95 equals max and is exploratory. Setup, validation and
abort are outside these action times.

| Workload | Action p50 B0 / B1 | Action p95=max B0 / B1 | Action IQR B0 / B1 | Median speedup |
| --- | --- | --- | --- | --- |
| Append | 4,571.507 / 16.424 | 5,919.054 / 91.850 | 1,203.747 / 23.307 | 278.34× |
| Scattered removal | 4,514.037 / 320.571 | 4,678.942 / 355.495 | 470.258 / 128.967 | 14.08× |
| Same-key replacement | 4,532.286 / 342.083 | 4,981.975 / 368.337 | 573.421 / 80.251 | 13.25× |

All three meet the diagnostic comparison to the existing timing target:
candidate median at most half control, with the gain exceeding both IQRs.
Source inspection supports the intended mechanism: current scalar operations
copy/freeze the keys order for each supplied change, while the candidate
constructs one order from removal lookup and insertion buckets. These timings
do not measure allocated bytes or satisfy the 65% allocation gate.

#### Separate setup and cleanup: unfavorable evidence retained

| Workload | Setup p50 B0 / B1 | Setup p95 B0 / B1 | Abort p50 B0 / B1 | Abort p95 B0 / B1 |
| --- | --- | --- | --- | --- |
| Append | 736.403 / 667.681 | 1,249.408 / 683.758 | 239.424 / 627.374 | 277.812 / 664.556 |
| Scattered removal | 720.841 / 706.729 | 755.305 / 768.200 | 0.130 / 0.081 | 0.139 / 0.277 |
| Same-key replacement | 697.194 / 664.162 | 1,377.097 / 683.606 | 0.120 / 0.083 | 0.175 / 0.206 |

**Append teardown is slower in the candidate.** Its median increases by
387.950 ms (+162.03%) against the unchanged 11.971 ms allowance. Teardown
IQRs are 23.774/328.675 ms. Its p95 increases by 386.744 ms against a
27.781 ms allowance. This unfavorable diagnostic cannot be hidden by the
action speedup or used as an acceptance pass. The teardown after complete
removal/replacement is cheap because old item watchers have already ended
during the action; append leaves all original watchers alive until abort.
That lifetime difference explains why append exposes cleanup work, not why
the candidate's abort is slower than control in that case.

Both implementations use the same release/deactivate/unlink path for abort;
the recorded traces cover only the 100-item setup case, not large append.
Heap/GC or runtime conditioning is a hypothesis, not an attributable cause.
No extra profile or run was added to investigate this new finding. Medians
from separate phases are not summed into a claimed full-lifetime median.

Read-only aggregation recomputed these summaries and verified the saved
report remained unchanged (hash
`20e1bc6503fffbfd409f053d139f498307d9d37836b9535ad2340eb5bad02c16`).
Original reports and numerical gates remain intact. Both phases are complete;
no adoption, public API, memory acceptance, independent confirmation or later
roadmap stage follows from these diagnostics.

### Stage 6: approved reduced comparison stopped at setup gate (2026-10-26 UTC)

**Outcome: reject the current prototype for adoption under this run's gate;
the cause of the setup tail remains unresolved.** The initial phase stopped
after 65.045 seconds on the Q03 setup-p95 failure, as the owner instructed.
The run completed three of twenty paired cases in Node. The other seventeen,
all Chromium cases, allocation/retention and independent confirmation are
unmeasured in this run, not passing. Earlier partial measurements remain
separate evidence with their original qualifications.

All values below are microseconds per action or per fresh setup/teardown,
summarized from fifteen measured **block means**. Nearest-rank p95 equals the
maximum at this sample count; it is not an individual-operation p95.

| Case | Action p50 B0 / B1 | Action p95 B0 / B1 | Setup p50 B0 / B1 | Setup p95 B0 / B1 | Setup IQR B0 / B1 |
| --- | --- | --- | --- | --- | --- |
| Q01: 1,000 items, ten removals, no subscriptions | 636.753 / 43.338 | 660.267 / 156.228 | 398.663 / 416.325 | 939.157 / 967.008 | 279.208 / 62.963 |
| Q02: 100 items, one mixed change, subscribed | 19.285 / 14.859 | 20.178 / 15.703 | 90.119 / 91.004 | 221.432 / 94.727 | 4.706 / 2.999 |
| Q03: 100 items, one removal, subscribed | 8.464 / 4.642 | 9.061 / 4.949 | 87.699 / 87.892 | 112.307 / 217.282 | 2.858 / 2.768 |

Q01's setup median increase was 17.662 µs, below its unchanged allowance of
19.933 µs. Q02's earlier action-p95 concern did not reproduce in this clean
set. Neither observation erases the original unfavorable samples or constitutes
independent confirmation.

Q03's setup-p95 increase was **104.975 µs (93.47%)** against an allowance of
**11.231 µs**, leaving a 93.744 µs excess, larger than either setup IQR.
The candidate's worst setup block was round 6; the control's was round 13.
Each was the mean of 100 complete setups. In the candidate's worst block,
collection construction averaged 88.838 µs and subscription setup 128.079 µs.
No block was excluded or attributed to GC without evidence.
Q03 teardown p95 also exceeded its limit: 54.213 versus 48.373 µs, a 5.841 µs
increase against a 4.837 µs allowance. Its 1.003 µs excess is smaller than
either teardown IQR (2.170/1.802 µs), so it is a separate unresolved gate,
not an additional clear regression claim. Q01/Q02 had no timing-gate failures.

#### Bounded diagnostics

Setup diagnostics took about 13.773 seconds, and p95 diagnostics 1.543 seconds;
both completed inside their investigation limits. Each setup regime used
five warmups and fifteen measured blocks of 100 constructions per variant.

| Setup preconditioning | End-to-end setup p50 B0 / B1 | Setup p95 B0 / B1 | Prebuilt-input construction p50 B0 / B1 |
| --- | --- | --- | --- |
| Matched canonical records | 493.543 / 534.497 | 628.060 / 629.194 | 347.262 / 349.888 |
| Matched empty/getter shapes | 614.082 / 519.157 | 873.994 / 650.087 | 387.920 / 401.004 |
| Old asymmetric semantic prefix | 527.164 / 599.600 | 606.156 / 818.231 | 295.579 / 395.257 |

The setup difference changes across regimes and common input/controller costs
also vary. This does not establish a consistent constructor penalty or prove
that asymmetric conditioning caused the original setup result. These are
diagnostics, not substitutes for the primary setup gates.

For Q02's diagnostic, block-mean action p95 B0/B1 was 24.011/22.497 µs with
subscriptions and counters off; 21.691/18.101 with counters on;
21.761/17.737 without subscriptions/counters; and 22.694/21.557 in the
counted-mode, unsubscribed control. Each has fifteen measured blocks of 100
actions. The counter-off subscribed individual-operation p95 was
27.338/22.430 µs over 1,500 actions, reported only as a diagnostic.
The original candidate action-tail regression did not reproduce. Fixed
diagnostic mode order and changing warm/heap state prevent interpreting these
values as a causal counter-overhead estimate. No GC-attribution pass was run.

The saved timing/calibration ledger accounts for 17.065 seconds of action,
14.970 seconds of validation, 14.493 seconds of reset and 0.132 seconds of
loop/unclassified work. These do not include the separate cold lifetimes,
diagnostics, validation-worker calls, reporting or coordination. The remainder
of the 65.045-second phase is not represented as product action time. Original
reports were not overwritten; read-only aggregation verified the new report
hash `2ee9b77ebcbc51f59223d5f637584871b02308edfc38a6f9b6812608953abce6`
and recomputed all three action summaries from saved block data.
Owned worker realms were terminated; cleanup reported no errors. No budget
was transferred or reset, and no rejected candidate was sent to confirmation.

### Stage 6: preserved-run time audit (2026-10-26 UTC)

This is read-only post-processing of the saved reports and inspection of the
saved runner source, not a new measurement run. Original reports were left
unchanged; their checksums matched before and after the audit. Existing raw
samples, partial/failed statuses, source identities and compact summary numbers
are preserved. The reduced matrix was a proposal when this audit was written;
the later approval and reduced run are recorded separately above.

| Portion of the initial 45-minute window | Recorded duration | What is established |
| --- | --- | --- |
| Expanded fresh-fixture calibration | 26m 16.975s | 139 completed paired cells; 34.112s inside action timers; 25m 42.863s unsegmented non-action work |
| Correction interval | 5m 29.863s | Time from stopping the first runner to starting its replacement; includes code/preparation, not product work |
| Corrected run | 13m 13.205s | 12 completed cells, plus an interrupted cell |
| Total | 45m 0.043s | Original deadline, including the correction; watchdog scheduling accounts for the final milliseconds |

Within the corrected run, the non-overlapping recorded components are:

| Component | Time |
| --- | --- |
| Warm action timers, including warmup blocks | 160.064s |
| Completed cells' calibration action timers | 5.023s |
| Complete cold lifetimes | 161.494s |
| Remainder, not phase-instrumented | 466.624s |

Cold lifetime already includes 40.699s of setup, 119.566s of action and 1.230s
of teardown; these must not be added again. The unsegmented remainders combine
preparation, reset, validation, preflight, reporting and incomplete work. The
old runner did not record enough timestamps to divide those costs reliably;
do not label the whole remainder subscription setup or GC.

The code and saved counts explain the avoidable multipliers:

- Fresh-fixture calibration rebuilt and released every subscription for every
  trial, rather than limiting calibration to a small representative set.
- Restoring large warmed fixtures used repeated ordinary remove/insert calls
  in both variants, so untimed reset itself repeated the quadratic keys work
  the candidate was intended to eliminate.
- A common repetition count was chosen from the faster variant. At 1,000-item
  full removal, 50 actions per block made a roughly 37 ms control repeat
  unnecessary work after its timing resolution was already adequate.
- The corrected run executed 5,633,840 hot-loop actions including warmups.
  Two equivalent empty-change aliases accounted for 3,884,640 of those calls
  (69% of calls, **not a measured percentage of elapsed time**).
- The saved counts imply 91,514,000 reference-map entries rebuilt outside
  action timing. Non-control actions also used repeated JSON serialization,
  including eager failure-message serialization even when checks passed.
  These are count-derived work estimates, not allocation-profiler results.
- Full semantic probes ran before each timing block in the same process;
  B1's additional getter/empty-record cases gave its construction helpers a
  different input-shape history. This is an identified conditioning asymmetry,
  not proof that it caused the setup difference; the bounded proposal retains
  the exact old condition and adds isolated, matched preconditioning regimes.

**Additional runner finding:** warmed fixtures requested counted subscriptions.
In subscribed cells, counter increments and membership-history pushes therefore
ran inside action timers, contrary to the planned uninstrumented boundary.
The two no-subscription cells installed no callbacks and did not execute this
instrumentation; their setup and noise findings cannot be attributed to it.
Retain all twelve summaries, favorable and unfavorable alike. Classify the
subscribed action numbers as **instrumented exploratory evidence**; the others
are unconfirmed initial observations. None establishes complete acceptance.
The p95 comparison needs a clean repeat plus a counter-on/off diagnostic, not
dismissal. Cold setup/lifetime samples used uncounted subscriptions, so this
finding does not dispose of the setup concern. No new measurement was taken
to infer these causes.

### Stage 6: amended calibration and bounded initial set (2026-10-26 UTC)

**Outcome: inconclusive for adoption.** The cap amendment worked. All 40
paired small calibration cells (80 variant cells) reached 20 ms in each engine:
Node used 990–30,000 actions per cell, Chromium 990–41,000, stopping at the
first completed chunk that reached the duration target. No cap or performance
gate was relaxed.

The unnecessary full fresh-fixture calibration extension completed 139/184
paired cells and accumulated 34.112 seconds of action time over 26 minutes
17 seconds of wall time before it was stopped as a runner correction. That is
a limitation of this driver procedure, not a candidate correctness failure.
Its data is retained. The corrected steady-state driver then used warmed,
restored fixtures, fresh correctness probes and separate cold measurements,
without resetting the initial deadline. It completed 12/184 paired timing
cells before the original 45-minute ceiling. The next cell, N = 100,
C = 1, scattered removal with subscriptions, was interrupted and is
unmeasured. Unstarted cells remain unmeasured too.

Each completed cell has five warmup blocks and fifteen measured blocks per
variant. The table reports quantiles of **per-action block means** in
microseconds, not individual-action or presented-latency percentiles. At
n = 15 the nearest-rank p95 is also the maximum and is exploratory. The
[compact summary](collection.json) contains n, p50, p95, maximum and IQR for
every completed action, collection setup, subscription setup, total setup,
teardown and complete-lifetime metric, recomputed from saved raw blocks.

The subsequent source audit found counted subscriptions in the subscribed
warmed action timings. Those values remain unchanged as instrumented
exploratory evidence. The two no-subscription action cells did not execute
counters, and cold setup/lifetime metrics were uncounted throughout. No
favorable or unfavorable number was removed because of this finding.

| N | C | Workload | Subscribed | B0 p50 / p95 / IQR, µs | B1 p50 / p95 / IQR, µs |
| --- | --- | --- | --- | --- | --- |
| 1000 | 10 | scatter-remove | No | 710.898 / 752.063 / 7.206 | 45.042 / 49.416 / 0.411 |
| 10 | 0 | replace | Yes | 0.361 / 0.373 / 0.007 | 0.496 / 0.523 / 0.026 |
| 10 | 2 | prepend | Yes | 2.289 / 2.879 / 0.089 | 2.528 / 2.651 / 0.073 |
| 100 | 100 | prepend | Yes | 115.551 / 127.909 / 9.453 | 57.700 / 65.667 / 4.283 |
| 10 | 0 | item-only | Yes | 0.700 / 0.709 / 0.006 | 0.691 / 0.701 / 0.008 |
| 1000 | 1000 | scatter-remove | No | 37292.338 / 124699.292 / 37304.266 | 411.146 / 3743.541 / 50.922 |
| 100 | 10 | scatter-insert | Yes | 13.014 / 15.698 / 1.378 | 12.331 / 14.040 / 0.624 |
| 100 | 10 | replace | Yes | 92.138 / 96.032 / 2.802 | 21.080 / 26.330 / 1.112 |
| 100 | 100 | append | Yes | 88.024 / 112.114 / 15.828 | 61.439 / 66.421 / 5.342 |
| 10000 | 10000 | mixed | Yes | 4345645.815 / 6485489.589 / 387024.963 | 28036.701 / 39288.241 / 1740.293 |
| 100 | 1 | mixed | Yes | 20.011 / 21.365 / 0.959 | 15.454 / 44.414 / 1.194 |
| 10 | 0 | scatter-insert | Yes | 0.366 / 0.385 / 0.011 | 0.528 / 0.712 / 0.039 |

The results show why a bulk path is worth investigating, but do not satisfy
the complete decision rule:

- Removing ten of 1,000 items without subscribers has a 710.898 → 45.042 µs
  initial median, with the difference larger than both IQRs.
- Removing all 1,000 items has a 37.292 → 0.411 ms median, about 90.7× by
  ratio. **The agreed noise criterion is not met in this set:** the control's
  37.304 ms IQR exceeds the 36.881 ms median difference. Do not turn the large
  ratio into a confirmed speedup claim.
- The 10,000-item full replacement and reverse with subscribers has a
  4,345.646 → 28.037 ms median, and its difference exceeds both IQRs. This
  C = N mixed cell has no surviving items to update; it measures membership
  replacement plus reorder, not a partial mixed action with survivor updates.
- Small completed action medians remain within their recorded allowances;
  for example the 10-item, two-entry prepend rises 2.289 → 2.528 µs.
  That does not establish unmeasured single-change or existing-operation cells.
- **Setup concern:** in the 1,000-item/ten-removal/no-subscriber cell, fresh
  setup rises 378.547 → 423.309 µs (+11.8%). Its 44.762 µs difference exceeds
  the 18.927 µs allowance and both setup IQRs (29.296/26.296 µs). This fails
  the initial setup limit; there is no confirmation set to establish scope.
- **p95 concern:** the subscribed 100-item, one-change mixed cell has an action
  block-mean p95 of 21.365 → 44.414 µs, beyond the 2.136 µs permitted increase.
  The median improves; it does not cancel this exploratory upper-percentile
  result. Independent confirmation is missing.

The 45-minute ceiling ended the set, not the 100,000-action cap. No extra
run was launched to replace unfavorable samples or recover omitted cells.
The timing matrix, independent confirmation, Chromium main timing, allocation
profiles, retained heap, churn, weak-reference release checks and shipping
confirmation remain incomplete or unrun. In particular, **the 65% allocation
gate and retained-memory gates have no measured result**. Source inspection
of the single-order construction is not allocation evidence.

The record retains the earlier smoke/calibration outcome and the stopped
runner attempt. No version of the prototype was tuned during these
measurements. No public API adoption, shipping implementation, package
verification suite, commit or stage 2 work follows from this partial result.

### Stage 6: smoke and early calibration stop (2026-10-26 UTC)

**Outcome: inconclusive performance; stop before main runs.** Each engine's
smoke exercised six workload groups at N = 100, 1,000 and 10,000, using
C = ceil(N/10), both without subscribers and with the specified item,
membership and membership-count subscriptions. Both variants matched the
independent order/value oracle, survivor identities and each other's delivery
counts in every smoke cell. These are single correctness samples per cell,
not latency samples or evidence that the performance gates hold.

| Workload | Membership callbacks/action | Item reads and callbacks/action | Wide reads/action | Wide callbacks/action |
| --- | --- | --- | --- | --- |
| Append, prepend, scattered insertion | 1 | 0 | 1 | 1 |
| Scattered removal | 1 | 0 | 1 | 1 |
| Same-key replacement | 1 | 0 | 1 | 0 |
| Mixed field updates, membership and reverse | 1 | C | 1 | 0 |

Counts exclude initial registration, apply to the subscribed fixture and were
identical for B0/B1 in Node/Chromium. All delivery counts were zero in the
unsubscribed fixture. Replacement reported each replaced key in both metadata
lists even though the key strings and order were unchanged. Both variants'
per-cell results and histories remain local; no raw samples are committed.

Calibration began with the mandatory cheap control, B0's empty batch on a
10-item collection without subscribers. Recomputed from its raw chunk
durations:

| Calibration observation | Value |
| --- | --- |
| Measured actions | 10,000, across ten chunks of 1,000 independent collections |
| Accumulated action time | 12.341604 ms |
| Approved minimum per block | 20 ms |
| Weighted action time | 1.2341604 µs/action; calibration only |
| Clock-pair sample | 10,000 pairs; p50 0.123 µs, p95 0.209 µs, max 417.65 µs |
| Resolution decision | Unresolved under the approved cap/minimum-duration procedure; stop |

This single calibration block cannot establish p50/p95 regression acceptance
or compare B1 with B0. The failure is in satisfying the planned measurement
boundary: it does not establish that the API is slow, fast, or incapable of
meeting the product thresholds. The owner requested stopping at this point;
there was no retry with relaxed rules.

**Source cost, not a speed result.** The unformatted candidate module is
41,132 bytes/1,099 lines against 36,779 bytes/994 lines for the control:
4,353 bytes/105 lines added. This includes the runtime method and import-path
adjustment, not final public typedefs, tests or guidance. Source inspection
shows one indexed original-order pass, traversal of insertion buckets, one
frozen next order and O(C) map/undo entries. This is algorithmic evidence,
not measured allocation evidence.

**Unmeasured:** B1 calibration, remaining Node calibration cells, Chromium
calibration, main timing and confirmation sets, standalone API timings,
allocation profiles, lifetime/retained-heap comparisons, forced-GC weak-reference
checks, churn, diagnostic D0 and shipping-path confirmation. The 2×/65%
membership-only gates, mixed-action regression gates and all memory gates are
unresolved. No unmeasured cell is treated as passing.

### Initial comparison (2026-10-25)

Medians across pages of each page's median, with the full-flow lab's
meaningful-difference rule.

| Measure                                  | Items          | Hand-written     | Lattice          | Difference         | Meaningful         |
| ---------------------------------------- | -------------- | ---------------- | ---------------- | ------------------ | ------------------ |
| Mount, µs per item                       | 10, 100, 1,000 | 17.0, 16.7, 20.4 | 19.2, 18.9, 23.1 | +2.2 to +2.7, +13% | Yes, except at 100 |
| Teardown, µs per item                    | 10, 100, 1,000 | 10.3, 10.1, 11.2 | 11.4, 12.0, 20.2 | +1.1, +1.9, +9.0   | Yes                |
| Single-item update, µs                   | 10, 100, 1,000 | 1.75, 1.94, 1.78 | 3.00, 16.5, 245  | +1.2, +14.6, +243  | Yes                |
| Reorder, µs per item                     | 10, 100, 1,000 | 2.28, 3.28, 12.0 | 2.53, 3.43, 12.1 | +11%, +4%, +1%     | Only at 10         |
| JavaScript heap per item, bytes          | 100, 1,000     | 575, 417         | 991, 926         | +417, +509         | Yes                |
| Bytes allocated per update, steady state | 10, 100, 1,000 | 77, 77, 77       | 276, 636, 4,249  | grows with items   | Yes                |

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

| Measure                                                   | Items          | Variant 1        | Variant 2        | Difference     | Meaningful    |
| --------------------------------------------------------- | -------------- | ---------------- | ---------------- | -------------- | ------------- |
| Teardown, µs per item                                     | 10, 100, 1,000 | 11.5, 12.6, 20.7 | 11.7, 12.1, 13.5 | +2%, −5%, −35% | At 1,000 only |
| Mount and teardown of 100 items, ms per cycle             | 100            | 4.11 (IQR 0.18)  | 3.87 (IQR 0.16)  | −6%            | Yes           |
| Retained after 20 cycles, bytes                           | 100            | 16,300           | 16,296           | 0%             | No            |
| Removing 900 of 1,000 items one at a time, µs per removal | 1,000          | 155              | 152              | −2%            | No            |
| Single-item update after that churn, µs                   | 100 remaining  | 20.3             | 20.6             | +1%            | No            |
| Heap after that churn, bytes                              | 100 remaining  | 1,774,772        | 1,781,136        | +0.4%          | No            |
| Mount, update and reorder                                 | 10, 100, 1,000 |                  |                  | −2% to +10%    | No            |

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

| Measure                                       | Items          | Shared map         | Store per item      | Relative         | Meaningful       |
| --------------------------------------------- | -------------- | ------------------ | ------------------- | ---------------- | ---------------- |
| Label update, µs                              | 10, 100, 1,000 | 3.81, 21.2, 339    | 2.68, 2.99, 5.20    | −30%, −86%, −98% | Yes              |
| Done toggle, µs                               | 10, 100, 1,000 | 3.63, 22.0, 342    | 2.84, 4.73, 31.8    | −22%, −79%, −91% | Yes              |
| Bulk label update of 10 items, µs             | 10, 100, 1,000 | 19.4, 39.5, 361    | 19.8, 19.9, 21.6    | +2%, −50%, −94%  | At 100 and 1,000 |
| Bulk label update of every item, µs           | 10, 100, 1,000 | 19.2, 220, 3,665   | 19.4, 194, 3,820    | +1%, −12%, +4%   | At 100 only      |
| Insertion at the end, µs                      | 10, 100, 1,000 | 34.3, 92.6, 849    | 30.3, 54.5, 355     | −12%, −41%, −58% | At 100 and 1,000 |
| Removal, µs                                   | 10, 100, 1,000 | 31.6, 125, 2,469   | 35.0, 64.3, 400     | +11%, −49%, −84% | At 100 and 1,000 |
| Reorder, µs per item                          | 10, 100, 1,000 | 2.64, 3.79, 12.6   | 2.44, 3.72, 12.7    | −8% to +1%       | No               |
| Mount, µs per item                            | 10, 100, 1,000 | 20.0, 20.8, 23.2   | 22.3, 21.0, 25.7    | +12%, +1%, +11%  | At 10 and 1,000  |
| Teardown, µs per item                         | 10, 100, 1,000 | 11.4, 11.9, 13.0   | 15.1, 15.4, 17.0    | +33%, +30%, +31% | Yes              |
| JavaScript heap per item, bytes               | 10, 100, 1,000 | 2,618, 920, 807    | 3,206, 1,692, 1,486 | +22%, +84%, +84% | Yes              |
| Bytes allocated per label update              | 10, 100, 1,000 | 268, 652, 4,253    | 85, 85, 85          | −68% to −98%     | Yes              |
| Bytes allocated per done toggle               | 10, 100, 1,000 | 247, 632, 4,231    | 114, 138, 138       | −54% to −97%     | Yes              |
| Bytes allocated per bulk update of every item | 10, 100, 1,000 | 700, 4,753, 44,993 | 986, 9,039, 89,424  | +41%, +90%, +99% | Yes              |

Selection reads and open-count derivations per operation, from the counted
pages (N items):

| Operation                    | Shared map reads | Shared map derivations | Store per item reads | Store per item derivations |
| ---------------------------- | ---------------- | ---------------------- | -------------------- | -------------------------- |
| Mount                        | 2N + 2           | 1                      | 3N + 2               | 1                          |
| Label update                 | 2N + 1           | 1                      | 1                    | 0                          |
| Done toggle                  | 2N + 1           | 1                      | 2                    | 1                          |
| Bulk label update of k items | 2N + 1           | 1                      | k                    | 0                          |
| Insertion                    | 2N + 4           | 1                      | 5                    | 1                          |
| Removal                      | 2N + 2           | 1                      | 2                    | 1                          |
| Reorder                      | 2                | 1                      | 2                    | 1                          |

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

| Measure                                            | Items          | Shared map         | Store per item         | Ratio               | Meaningful |
| -------------------------------------------------- | -------------- | ------------------ | ---------------------- | ------------------- | ---------- |
| Bulk done update of 10 items, µs                   | 10, 100, 1,000 | 21.4, 40.6, 356    | 29.0, 43.1, 189        | 1.36×, 1.06×, 0.53× | Yes        |
| Bulk done update of every item, µs                 | 10, 100, 1,000 | 20.6, 240, 4,245   | 28.0, 443, 21,065      | 1.35×, 1.84×, 4.96× | Yes        |
| Bytes allocated per bulk done update of every item | 10, 100, 1,000 | 928, 7,874, 79,808 | 1,448, 16,285, 162,512 | 1.56×, 2.07×, 2.04× | Yes        |

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
| ----- | ------------- | ------------------- | ----------------- | ------------------- | ---------- |
| 100   | 922           | 920                 | 1,693             | 1,434               | 527        |
| 1,000 | 811           | 806                 | 1,486             | 1,335               | 521        |

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

| Measure                                             | Items          | Shared map         | Store per item         | Keyed collection       | Keyed vs shared map | Meaningful       |
| --------------------------------------------------- | -------------- | ------------------ | ---------------------- | ---------------------- | ------------------- | ---------------- |
| Label update, µs                                    | 10, 100, 1,000 | 3.85, 21.4, 339    | 2.80, 3.08, 5.22       | 2.81, 3.17, 4.92       | −27%, −85%, −99%    | Yes              |
| Done toggle, µs                                     | 10, 100, 1,000 | 3.70, 22.1, 341    | 2.88, 4.79, 31.5       | 3.14, 6.09, 45.9       | −15%, −72%, −87%    | Yes              |
| Bulk label update of 10 items, µs                   | 10, 100, 1,000 | 20.1, 39.7, 358    | 20.3, 20.2, 21.8       | 21.3, 22.8, 22.8       | +6%, −42%, −94%     | At 100 and 1,000 |
| Bulk label update of every item, µs                 | 10, 100, 1,000 | 19.4, 222, 3,680   | 19.8, 198, 3,800       | 20.7, 220, 3,450       | +7%, −1%, −6%       | At 10 only       |
| Bulk done update of 10 items, µs                    | 10, 100, 1,000 | 20.3, 40.6, 356    | 27.5, 42.8, 257        | 22.2, 25.8, 64.3       | +9%, −36%, −82%     | Yes              |
| Bulk done update of every item, µs                  | 10, 100, 1,000 | 20.4, 237, 3,945   | 27.5, 442, 23,275      | 22.0, 233, 3,635       | +8%, −2%, −8%       | At 10 and 1,000  |
| Insertion, µs                                       | 10, 100, 1,000 | 33.0, 87.4, 849    | 31.1, 53.0, 327        | 25.3, 49.4, 252        | −23%, −43%, −70%    | Yes              |
| Removal, µs                                         | 10, 100, 1,000 | 31.8, 124, 2,469   | 36.3, 64.5, 371        | 26.1, 58.9, 347        | −18%, −53%, −86%    | Yes              |
| Reorder, µs per item                                | 10, 100, 1,000 | 2.63, 3.77, 12.6   | 2.59, 3.77, 12.5       | 2.46, 3.57, 12.4       | −6% to −1%          | No               |
| Mount, µs per item                                  | 10, 100, 1,000 | 21.0, 22.0, 23.4   | 22.2, 20.9, 25.9       | 21.4, 18.7, 23.5       | +2%, −15%, +1%      | No               |
| Teardown, µs per item                               | 10, 100, 1,000 | 11.9, 13.4, 12.9   | 15.3, 15.6, 16.8       | 11.5, 11.4, 12.8       | −4%, −15%, −2%      | No               |
| JavaScript heap per item, bytes                     | 10, 100, 1,000 | 2,636, 922, 809    | 3,221, 1,693, 1,487    | 3,761, 1,012, 897      | +43%, +10%, +11%    | Yes              |
| Bare container per item, bytes                      | 10, 100, 1,000 | 263, 69, 55        |                        | 596, 150, 119          | +126%, +117%, +119% | Yes              |
| Bytes allocated per label update                    | 10, 100, 1,000 | 268, 652, 4,253    | 86, 86, 85             | 57, 57, 57             | −79% to −99%        | Yes              |
| Bytes allocated per done toggle                     | 10, 100, 1,000 | 247, 631, 4,232    | 114, 138, 138          | 86, 110, 110           | −65% to −97%        | Yes              |
| Bytes allocated per bulk label update of every item | 10, 100, 1,000 | 701, 4,757, 44,983 | 983, 9,044, 89,559     | 1,716, 16,535, 171,439 | +145% to +281%      | Yes              |
| Bytes allocated per bulk done update of every item  | 10, 100, 1,000 | 678, 4,709, 45,202 | 1,446, 16,291, 162,641 | 1,507, 16,557, 171,589 | +122% to +280%      | Yes              |

Selection reads, open-count derivations and summary writes per operation,
from the counted pages (N items, k items changed):

| Operation                                 | Shared map                           | Store per item | Keyed collection |
| ----------------------------------------- | ------------------------------------ | -------------- | ---------------- |
| Label update                              | 2N + 1 reads, 1 derivation, 0 writes | 1, 0, 0        | 1, 0, 0          |
| Done toggle                               | 2N + 1, 1, 1                         | 2, 1, 1        | 2, 1, 1          |
| Bulk label update of k items              | 2N + 1, 1, 0                         | k, 0, 0        | k, 0, 0          |
| Bulk done update of k items               | 2N + 1, 1, 1                         | 2k, k, k       | k + 1, 1, 1      |
| Insertion                                 | 2N + 4, 1, 1                         | 5, 1, 1        | 3, 1, 1          |
| Removal                                   | 2N + 2, 1, 1                         | 2, 1, 1        | 1, 1, 1          |
| Reorder                                   | 2, 1, 0                              | 2, 1, 0        | 1, 1, 0          |
| Remove and reinsert one key in one action |                                      |                | 3, 1, 1          |

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

### Stage 5: shipping path (2026-10-25)

Variant 5, the shipping collection imported from `@canonic2/lattice/collection`,
against variant 1, the shared map, with variant 3, store per item, and variant
4, the prototype, as references. Medians across pages of each page's median;
"meaningful" uses the stage 1 rule against the shared map.

| Measure                                             | Items          | Shared map         | Store per item    | Prototype              | Shipping               | Shipping vs shared map | Confirmation: shared map, shipping |
| --------------------------------------------------- | -------------- | ------------------ | ----------------- | ---------------------- | ---------------------- | ---------------------- | ---------------------------------- |
| Label update, µs                                    | 10, 100, 1,000 | 3.84, 21.3, 341    | 2.79, 3.22, 5.35  | 2.89, 3.19, 5.06       | 2.83, 3.19, 4.96       | −26%, −85%, −99%       | 3.87, 21.8, 342; 3.00, 3.09, 4.89  |
| Done toggle, µs                                     | 10, 100, 1,000 | 3.69, 21.7, 343    | 2.87, 4.87, 32.1  | 3.13, 6.13, 45.1       | 3.11, 6.22, 45.9       | −16%, −71%, −87%       | 3.74, 22.1, 343; 3.25, 6.13, 46.8  |
| Bulk label update of 10 items, µs                   | 10, 100, 1,000 | 20.1, 39.5, 360    | 20.0, 21.6, 21.8  | 21.9, 22.5, 23.1       | 21.6, 22.5, 23.0       | +8%, −43%, −94%        | 20.0, 39.7, 362; 22.7, 22.0, 23.5  |
| Bulk label update of every item, µs                 | 10, 100, 1,000 | 19.8, 226, 3,670   | 19.6, 205, 3,920  | 21.2, 213, 3,555       | 21.0, 216, 3,470       | +6%, −4%, −5%          | 19.9, 219, 3,615; 22.2, 214, 3,330 |
| Bulk done update of 10 items, µs                    | 10, 100, 1,000 | 20.5, 40.0, 358    | 27.7, 43.1, 281   | 22.6, 26.2, 64.4       | 22.5, 26.4, 63.9       | +10%, −34%, −82%       | 20.7, 40.2, 357; 23.2, 26.3, 64.9  |
| Bulk done update of every item, µs                  | 10, 100, 1,000 | 20.4, 235, 3,915   | 27.4, 456, 25,075 | 22.1, 230, 3,670       | 22.4, 237, 3,470       | +10%, +1%, −11%        | 20.3, 236, 3,945; 23.0, 232, 3,590 |
| Insertion, µs                                       | 10, 100, 1,000 | 34.0, 89.2, 860    | 29.1, 55.7, 332   | 27.0, 49.0, 255        | 25.1, 50.1, 253        | −26%, −44%, −71%       | 34.2, 90.1, 892; 29.1, 49.0, 260   |
| Removal, µs                                         | 10, 100, 1,000 | 33.2, 127, 2,472   | 35.2, 67.9, 385   | 27.6, 58.3, 344        | 26.5, 58.8, 345        | −20%, −54%, −86%       | 33.4, 127, 2,532; 29.0, 58.0, 353  |
| Reorder, µs per item                                | 10, 100, 1,000 | 2.67, 3.76, 12.7   | 2.61, 3.93, 12.7  | 2.48, 3.52, 12.5       | 2.45, 3.60, 12.4       | −8%, −4%, −2%          | 2.63, 3.78, 12.8; 2.54, 3.57, 12.7 |
| Mount, µs per item                                  | 10, 100, 1,000 | 20.3, 20.4, 23.4   | 22.4, 22.3, 26.3  | 21.5, 18.9, 22.9       | 21.5, 19.1, 22.7       | +6%, −7%, −3%          | 20.9, 22.3, 23.3; 22.4, 18.8, 23.1 |
| Teardown, µs per item                               | 10, 100, 1,000 | 11.6, 11.6, 13.1   | 15.2, 16.4, 17.1  | 11.8, 11.5, 13.0       | 11.7, 11.5, 13.1       | +1%, −1%, +1%          | 11.7, 13.3, 12.9; 12.1, 11.4, 13.1 |
| JavaScript heap per item, bytes                     | 10, 100, 1,000 | 3,184, 922, 809    |                   | 3,761, 1,012, 897      | 3,845, 1,085, 898      | +21%, +18%, +11%       |                                    |
| Bare container per item, bytes                      | 10, 100, 1,000 | 271, 70, 55        |                   | 604, 151, 119          | 614, 152, 119          | +127%, +117%, +119%    |                                    |
| Bytes allocated per label update                    | 10, 100, 1,000 | 268, 652, 4,253    |                   | 57, 57, 57             | 57, 57, 57             | −79% to −99%           |                                    |
| Bytes allocated per done toggle                     | 10, 100, 1,000 | 247, 632, 4,232    |                   | 86, 110, 110           | 85, 110, 110           | −65% to −97%           |                                    |
| Bytes allocated per bulk label update of every item | 10, 100, 1,000 | 700, 4,762, 44,987 |                   | 1,718, 16,568, 171,318 | 1,721, 16,558, 171,273 | +146% to +281%         |                                    |
| Bytes allocated per bulk done update of every item  | 10, 100, 1,000 | 678, 4,716, 44,940 |                   | 1,507, 16,568, 171,270 | 1,433, 16,581, 171,329 | +111% to +281%         |                                    |

The shipping collection matches the prototype within run-to-run variation in
every timing cell, and to the byte in allocations. Heap per item at 100 items
is 73 bytes above the prototype, about the 61-byte interquartile range of that
cell; at 1,000 items they are equal.

**Garbage collection**, from the traced runs: medians across 10 pages, per run
of 20,000 label updates, 20,000 done toggles, or a bulk done update of every
item repeated 20,000 ÷ N times (2,000, 200 and 20 calls at 10, 100 and 1,000
items, each about 3.4 MB of allocation with the shipping collection).

| Workload                       | Items          | Shared map: collections, ms | Shipping: collections, ms |
| ------------------------------ | -------------- | --------------------------- | ------------------------- |
| Label updates                  | 10, 100, 1,000 | 1, 1.3; 3, 2.1; 21, 9.0     | 1, 1.3; 1, 1.2; 0, 0      |
| Done toggles                   | 10, 100, 1,000 | 1, 0.5; 3, 1.1; 10, 4.2     | 0, 0; 0, 0; 0, 0          |
| Bulk done update of every item | 10, 100, 1,000 | 1, 0.4; 0, 0; 0, 0          | 1, 0.4; 1, 0.4; 0, 0      |

Every collection was a minor one; no run in either variant had a major
collection. The shipping collection's bulk updates triggered at most one minor
collection of about 0.4 ms per 3.4 MB run; single-item updates, which allocate
57 to 110 bytes, triggered none at 1,000 items, where the shared map's copy of
the map triggered one collection every 1,000 to 2,000 updates.

Counted pages: row DOM effects of the shipping collection were identical to
the shared map's at every step and size, and every count, including `read`s,
open-count derivations and summary writes, was identical to the prototype's.

## Correctness

### Stage 6: independent memory and regular cleanup

Semantic/counting preflights used the actual experimental export and frozen B0
for all selected workload groups, outside profiling. Completed action profiles
passed post-window value/order/identity checks. Lifetime profiles checked their
bounded results after sampling stopped. Retained commit/rollback calls verified
canonical outcomes and rollback identities before dropping temporary oracles.
Caller holds existed until explicitly released. All four weak-reference groups
released their targeted objects in all five observations, and all four churn
groups completed 100 cycles. No product correctness error stopped the phase.
The warming/cancellation defects are runner corrections; affected data is not
claimed as acceptance.

The independent regular collection/store fixes have work-count, original-error,
live-signal detach and ended-callback regression cases in their owning test
files. This is not a claim that the broader package/type/browser suites ran.

### Stage 6: experimental package checkpoint

The lab adapter loads the frozen baseline and actual experimental package
entry. Shared/candidate semantic probes, counted preflights and all executed
post-timer checks passed in both initial/corrected teardown reproductions and
the two admission cases. Checks cover frozen/stable keys, unchanged old arrays,
record values/order, survivors, replacement identities, expected delivery
counts, errors and lifecycle ownership. No correctness failure stopped a set.
The corrected abort callback was exercised through the actual package export;
live-signal removal paths remain separate from once-dispatch cleanup.

The authored Vitest suite and packed/type fixtures map additional checkpoint
contracts to runnable tests. This record does not treat authored or partial
test coverage as a full executed package suite. Release by forced-GC probes,
retained memory, allocation and cross-engine acceptance remain unverified.

### Stage 6: eight-minute diagnostic amendment (2026-10-26 UTC)

The separate validation process passed the shared and candidate semantic
probes, the exact Q03 lifecycle, and counted small-size append/removal/
replacement probes with equivalent delivery counts. The expected-error and
owned-child cancellation probes completed before the measurement set.
All measured setup/removal lifetimes and all 54 bulk warmup/measured actions
passed the executed post-timer checks: exact order/values, stable/frozen
keys, unchanged old keys, survivor references, replacement record identities
and no reported delivery errors. Full removal and replacement ended old
item watchers at commit; retained append watchers ended at owned abort.

No correctness or runner failure occurred. All child processes were closed,
with no stderr or cleanup errors. No forced-GC release, allocation, retained
heap, churn or cross-engine acceptance was performed; GC tracing alone does
not establish those gates.

### Stage 6: approved reduced run (2026-10-26 UTC)

Both variants passed the exercised semantic probes (seven shared probes and
three additional candidate validation/anchor probes), repeated in the isolated
validation realm before each timing block. All actions in Q01–Q03 passed
post-timer exact order/value, frozen/stable keys, unchanged old keys, survivor
identity, replacement-incarnation and reported-error checks. No correctness
failure ended this run.

Separate counted preflights, repeated over three restored actions, matched:
Q01 delivered nothing without subscriptions; Q02 delivered membership once,
one updated item's read/callback and one collection-wide read, suppressing its
unchanged size callback; Q03 delivered membership and size once with no item
read/callback. All counted work was outside acceptance action timings.
Large workloads and the seven named small controls did not reach preflight
or measurement in this run. Forced-GC release, retained heap, churn and
allocation acceptance were not run. Exercised semantic cleanup does not
establish those unmeasured memory gates.

### Stage 6: resumed evaluation (2026-10-26 UTC)

No candidate correctness failure was observed. Both resumed calibration
processes passed their common and bulk-specific probes. Each completed
corrected timing cell passed fresh-fixture probes and three restored-fixture
comparisons for each variant before sampling. Restored delivery counts matched
between variants, including item readers and callbacks, membership and
collection-wide reads/callbacks. Exact values, order and survivor identities
were checked after every measured action; the semantic probe sequence ran
before each measured block. Restoration used public operations outside the
timer and re-established subscriptions for replaced original incarnations.

That is bounded observed coverage, not completed acceptance. Forced-GC release,
retained heap and churn were not run; the interrupted cell has no complete
saved sample and is not passing coverage. The initial extended-calibration
attempt was classified as a runner procedure defect, not a product failure.

### Stage 6: smoke correctness (2026-10-26 UTC)

Both variants passed the common probes in Node and Chromium: successive reads
inside ordinary batches; frozen old arrays and survivor identities; exact
nested and outer rollback; same-key incarnation replacement and ending old
watchers; transient incarnation/no-op batches; aborted subscriptions staying
ended after rollback; committed removal detaching a live signal listener;
queued reentrant writes and late registration; failed initial callbacks and
batch subscription refusal; abort during delivery; failed callbacks retaining
their last successful value; isolated read and run errors.

B1 also passed atomic rejection of missing/duplicate/conflicting keys, null
change arrays and invalid items; invalid calls caught after earlier outer
writes; empty input preserving keys identity; read-once item getters observing
pre-call state with reentrant writes refused; throwing-getter atomicity; and
stable insertion across multiple surviving-anchor groups and appends.

The independent oracle and counted smoke comparisons passed in both engines
for all exercised workloads and sizes. No correctness failure or runner defect
was observed. This is bounded smoke coverage, not completed correctness
acceptance: forced-GC release/retention, full small-operation coverage and the
remaining measured-sample probes were not run after calibration stopped.
Chromium reported no page errors; browser cleanup completed.

### Initial comparison

| Variant         | Counted pages | Failed checks |
| --------------- | ------------- | ------------- |
| 0. Hand-written | 9             | None          |
| 1. Lattice      | 9             | None          |

Both passed, at 10, 100 and 1,000 items: rows matched the items after mount;
an update changed exactly one row; a reorder reversed the rows by moving them
without creating or removing any; after teardown no row remained, every
observer was disconnected and listener and node counts returned to their
baseline.

### Stage 3

| Variant           | Counted pages | Failed checks |
| ----------------- | ------------- | ------------- |
| 1. Shared map     | 9             | None          |
| 3. Store per item | 9             | None          |

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

| Variant             | Counted pages | Failed checks |
| ------------------- | ------------- | ------------- |
| 1. Shared map       | 9             | None          |
| 3. Store per item   | 9             | None          |
| 4. Keyed collection | 9             | None          |

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

### Stage 5

| Variant                | Counted pages | Failed checks |
| ---------------------- | ------------- | ------------- |
| 1. Shared map          | 9             | None          |
| 4. Prototype           | 9             | None          |
| 5. Shipping collection | 9             | None          |

The shipping collection passed every stage 4 check at every size, including
the summary limits and key reuse. Before the runs, the package's own tests of
the shipping implementation passed: the collection's unit tests, which cover
the contract's acceptance areas, the store's unit tests after the shared
reporter moved, the Node import test of the new entry point, and the
packed-archive test with the collection's consumer type fixtures in both
environments.

## Threats to validity

### Stage 6: independent memory evidence

Five allocation profiles are sampled estimates, not exact totals. Observed
profile ranges are descriptive uncertainty, not confidence intervals. C02's
range is beyond its gate; Q03 crosses its gate and is inconclusive. Narrow
passing margins, including C03, need independent confirmation. Flawed fresh
Q/C action profiles remain exploratory regardless of outcome; no unfavorable
cell was silently dropped or candidate tuning performed mid-run.

Forced-GC used-heap readings include runtime, compiled code and probe state.
Warmup did not settle all helpers/branches before the first empty snapshot.
Both variants show residuals in empty controls and further stepwise churn
growth. Byte-budget failures are therefore not attributable solely to product
retention. Targeted weak-reference release cannot waive them or establish that
every collection object is unreachable. No retainer snapshot isolates the
residue here. All failed replicates and spikes remain evidence; a bounded
calibration/retainer investigation is required before another large acceptance
set. Do not keep warming or collecting until a favorable baseline appears.

Setup CPU profiles contain GC samples while the event observer returned no
entries before capture, so its event/time correlation is unavailable. Shared
profile-order effects and near-equal setup allocations do not explain the
original admission p95 tail. High host load and profiling perturbation remain
limits. Memory completion does not imply timing admission or API acceptance.

### Stage 6: checkpoint admission and cleanup attribution

The repeated abort improvement is supported by a work-count reproduction,
before/after source correction, profiles without observed GC during abort,
pre-GC controls and three-process repetition through the package entry. The
mechanism is shared redundant once-listener removal; the experiment does not
fully isolate the lower-level reasons native searches cost more in the old
candidate heap than the baseline heap. The short corrected CPU profile has
too few samples for precise component attribution.

Admission setup p95 is the maximum of fifteen block averages, not a robust
individual-operation percentile. Load rose from 10.13 to 21.39 during that
set. The setup regression is a gate failure with unresolved cause, not proof
of a persistent intrinsic constructor penalty and not a reason to waive the
gate. Missing cases, memory evidence and confirmation cannot be inferred from
the targeted abort improvement. The default entry deliberately retains the
accepted implementation until a separate acceptance decision.

### Stage 6: eight-minute diagnostic amendment

Initial load average 60 on 16 logical CPUs makes host contention a material
limit. Strict interleaving and independent processes preserve that variation;
they do not remove it. The primary setup p95 is a percentile of 200 block
averages of 100 setups, not the latency of an individual setup. The selected
ten-block bootstrap cannot characterize pauses correlated over longer runs;
its wide intervals prevent confident causal attribution. Do not discard
process 1's failure, process 3's maximum or the original failed result.

Bulk has seven actions per variant in one process per operation. Its large
median gains exceed the observed IQRs but remain diagnostic, without an
independent confirmation, allocation or retention evidence, small-operation
coverage, mixed actions or Chromium measurements in this amendment. Candidate
append abort is slower and untraced; there is no supported cause for that
regression. The separately instrumented setup passes cannot explain a
different large-workload tail. No complete-lifetime median was measured.

### Stage 6: reduced run tail evidence

The stop is a failure of the agreed acceptance statistic, not proof of a
persistent candidate-specific setup regression. Fifteen block means make p95
the maximum; the setup failure is dominated by one candidate block while
setup medians are nearly equal. A different control block also has a setup
tail. Shared host load, runtime/heap conditioning and GC remain possible
causes; none was instrumented or removed from decision samples. Timer
resolution was not separately calibrated in this run, and the optional
GC-attribution diagnostic was not collected. The original and new unfavorable
samples remain visible. No independent confirmation was performed after the
gate-triggered stop, as required by the owner's latest instruction.

The large fresh-block path and independent large-case counts were not reached.
Their implementation therefore provides no performance or acceptance evidence.
The executed Q01–Q03 cold paths have contiguous setup/action/abort boundaries,
with validation after the timer. Future use of the fresh-block helper must
distinguish its recorded reference-capture gap from a clean complete-lifetime
boundary; its unexercised span includes that gap and cannot be claimed as the
planned clean lifetime measurement without correction before use.

- The resumed stage 6 initial set is incomplete: 12/184 paired cells, one
  process and no independent confirmation. The initial unnecessary extension
  to full fresh-fixture calibration consumed most of the budget; preserve
  that runner limitation when assessing what remains unmeasured.
- Warm reset uses public operations outside timing, retains initial model
  references for identity comparison and re-subscribes replaced incarnations.
  Its heap/cache/registration history differs from a newly constructed
  collection. Fresh correctness and cold lifecycle metrics are separate;
  warm action numbers do not establish cold-action or retained-memory costs.
- Main p95 values are quantiles of block means, not single-operation tails.
  At fifteen blocks the reported p95 equals the maximum. The shared host's
  higher load, natural GC and unavailable power state limit interpretation;
  the large control IQR in full removal fails the agreed meaningful-win rule.
- Stage 6 stopped in calibration: one empty-control block in Node, no paired
  candidate timing, no main samples or profiling. The control did not reach the
  approved minimum accumulated duration before its action cap; this is a
  procedure-resolution limit, not a measured failure of the API. Smoke covers
  one change count per size, not the full approved matrix. No performance,
  allocation, leak-freedom or readiness claim follows from those smoke probes.
  Uncollected power state, shared-host load and timer outliers further limit
  interpretation of the calibration value.
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
- Stage 5's load rose to a load average of 67 late in the confirmation set and
  stayed between 70 and 88 at the start of the counted and memory sets. The
  confirmation agrees with the first timing set, which ran at 6 to 9, in every
  threshold; work counts and heap measures do not depend on load.
- Garbage collections are sparse events: at most a few per traced run, so the
  collection figures show their scale, not a precise rate. Young-generation
  sizing in other hosts and with other application heaps changes when they
  happen.

## Decision and follow-ups

### Stage 6: attributable append correction, calibration still unresolved

Keep the API experimental and unaccepted. The regular API retains only the
independent cleanup corrections in this commit series; previous working-tree
API changes are outside it. The new single-append diagnostic range fits the
unchanged allocation allowance, with behavioral coverage for the affected
validation/identity/rollback path. This is not a completed timing, lifetime,
retention or full-matrix confirmation of the corrected revision.

The retention instrument detects an intentionally held object, but a shared
first-use footprint and a repeated approximately 32 KiB baseline shift prevent
reliable resolution of the 16 KiB gate. Before a substantial acceptance run,
isolate that shift with matched heap/retainer evidence and stable positive
controls; predeclare workload-specific warmup rather than warming until a
passing baseline appears. The original failures remain recorded. Setup-tail
attribution is still open. No public API adoption follows from these fixes.

### Stage 6: independent memory reported, regular cleanup fixed

**Keep bulk membership experimental and unaccepted.** Independent memory
completed all selected cases without waiving setup admission. Large action
allocations fit the 65% reduction target and large lifetime allocations fit
their no-regression limits. A ten-item single-append bulk call fails the
256-byte additional-allocation allowance; a 100-item single removal remains
inconclusive at the approved five-profile scope.

Retained-heap acceptance remains unestablished: live-overhead numerical failures,
shared post-drop residue and churn growth remain visible. Weak probes show
targeted release, while runtime/probe/code footprint has not been isolated from
product retention. Confirmation remains unrun; neither memory nor setup profiles
override setup p95. No additional samples, relaxed limit or later stage follows.

The accepted regular collection now avoids redundant once-listener searches
on abort; the store avoids redundant detach after an initial callback aborts
then throws. Those independently authorized fixes and regression coverage do
not depend on experimental API adoption. Frozen comparison sources are intact.

Remaining engineering decisions: reduce small bulk-call allocation overhead,
calibrate and attribute retained-heap residuals before another acceptance run,
and resolve setup-tail attribution under controlled conditions. Do not confuse
the completed evidence collection with acceptance of its result.

### Stage 6: reviewable checkpoint, cleanup fixed, admission incomplete

**Implemented/experimental, not accepted.** The checkpoint is in reviewable
source behind `@canonic2/lattice/experimental/collection`, with correctness
and consumer fixtures. The regular collection code/export/instance surface
is unchanged. Original measured baseline, reporter, prototype, initial
checkpoint and corrected checkpoint are separately preserved; measurements
record which revision actually ran.

The attributable abort inefficiency is corrected only in the checkpoint and
the same comparison verifies it. Setup acceptance remains unresolved: the
corrected checkpoint admission failed Q02 setup p95, so the agreed stop rule
prevented allocation/retention and confirmation from starting. Their budgets
were not spent, reset or repurposed. No API adoption or supported-API promise
was introduced, and no later workload stage started.

The remaining decision is how to resolve the setup-tail gate under comparable
host conditions without discarding its failed evidence. Continuing memory
or confirmation despite this admission failure would change the agreed
sequencing/stop rule and requires an explicit amendment; it is not implied
by the prepared checkpoint. Keep the checkpoint available for review while
that question remains open.

### Stage 6: eight-minute diagnostic outcome, prototype unadopted

The approved amendment completed both phases without expanding its matrix
or calibrating repetitions. Setup p95 exceeds its allowance in one of three
fresh processes: **inconsistent/unresolved** under the precommitted label.
The original failed adoption gate is preserved. Bulk append/removal/replacement
deliver the expected diagnostic action benefit, exceeding the halving and
meaningful-gain timing criteria in all three selected cases.

No candidate-specific cause of the setup tail is established. GC overlap is
observed in both instrumented variants, while component differences and
primary process outcomes vary. Large append also exposes a separate slower
candidate abort/teardown, whose cause remains unmeasured. Keep both unfavorable
observations visible; action gains cannot override either.

**Keep the prototype unadopted.** Adoption still lacks resolved setup/cleanup
behavior, allocation/retention acceptance and independent confirmation.
The remaining questions are specific: whether the setup tail persists under
comparable low-contention conditions, and what causes the slower candidate
abort after large append. No further measurement or matrix expansion is
authorized by completing this amendment. No product code was changed, no
shipping comparison was implied, and no later stage was started.

### Stage 6: reduced comparison gate stop, no adoption

**Decision-rule outcome: reject the current candidate for adoption on this
run.** Q03 failed the unchanged setup-p95 allowance by more than either IQR;
the owner explicitly instructed stopping when a clear gate failure made
confirmation unwarranted. No allocation, retention or confirmation budget was
spent. This rejects neither the `updateMembership` API shape nor the bulk
membership engineering direction. There is no owner adoption decision and no
shipping API change.

The specific unresolved question is whether Q03's setup tail is reproducible
and attributable to the candidate's construction/registration footprint, or
to runtime/heap/host variation under the fixed boundary. The separate teardown
tail also remains above its numerical gate but below its IQR uncertainty.
Q01/Q02 passing this set does not retroactively clear their earlier evidence.
Any renewed evaluation needs a bounded, agreed investigation of those tails
before another large comparison; do not quietly exclude the failed block or
relax its p95 gate. The unexercised fresh lifetime helper also needs the
boundary correction described under Threats before use.

All seventeen unfinished selected cases, all Chromium comparisons and every
deferred original cell remain unmeasured in this reduced run. Allocation,
retention, independent confirmation and a shipping comparison remain absent.
Stage 1 stays open; no later stage was started. Retain all raw reports and
the unchanged prototype/control for a future explicitly scoped decision.

### Stage 6: reduced comparison approval

The proposed 20-case amendment is fixed before further runs. It explicitly
changes matrix coverage, large-case fixture/repetition procedure, allocation
action counts and churn coverage while keeping every acceptance threshold and
the original phase ceilings. It retains the unfavorable setup, p95 and noisy
removal cases. Raw partial reports remain unchanged; the saved time audit and
source inspection add explanation, not new performance evidence.

The owner agreed this reduction on 2026-10-26 UTC before measurements,
including stopping on a clear adoption-gate failure. No candidate revision,
adoption, shipping implementation or later stage is implied. The agreed items
are the 20 selected cases,
fresh large-case comparisons with independent calibrated counts, one-action
large allocation profiles with five independent replicates, the two named
100-cycle churn sentinels, and the 25–35/30–50/20–30 minute phase estimates.

### Stage 6: bounded initial evaluation, no adoption

**Decision-rule outcome: inconclusive overall. Recommendation: do not adopt or
ship on this evidence.** The prototype passed the exercised correctness probes
and has substantial median gains in completed membership-heavy cells. It has
not met the complete acceptance rule: the meaningful-win criterion is
unresolved in the sampled 1,000-item full-removal cell, an initial setup result
exceeds its limit, a p95 result exceeds its limit, most timing cells are missing
and no allocation or retained-memory evidence exists.

| Gate | Evidence and remaining scope |
| --- | --- |
| Correctness, identities, rollback and cleanup behavior | Observed probes passed in Node/Chromium and completed warm/fresh cases; forced-GC release acceptance remains unrun |
| 2× membership-only speedup and meaningful difference | Partial medians only; full-removal control IQR fails the precommitted noise test, other required large cells are missing |
| Mixed-action non-regression | Completed medians improve; the 100-item/one-change block-mean p95 exceeds its allowance and needs confirmation |
| Cheap operations, setup and teardown | Completed action medians fit their allowances; one setup regression exceeds both the allowance and IQRs; unmeasured controls are not passing |
| 65% allocation reduction, small-operation allocation ceiling | Unmeasured |
| Live retained heap, post-teardown growth, churn | Unmeasured |
| Independent confirmation and shipping comparison | Unrun; there is no accepted shipping implementation |

The cap amendment does not need another approval: it resolved its intended
calibration problem. The initial comparison nevertheless reached its unchanged
45-minute ceiling after a runner correction, so unfinished cells were stopped
and retained as incomplete. Separate confirmation/profiling budgets were not
used to extend that set and were not exhausted. No source, metric or gate was
changed to convert these results into acceptance.

Concrete remaining work is to confirm/isolate the setup and p95 concerns,
complete the missing timing comparisons with the corrected bounded driver,
and measure allocations plus retained memory/release. Any candidate revision
must keep the original baseline, failed/partial evidence and unchanged gates
visible, with a dated method amendment before measuring. The owner has not
adopted `updateMembership`; public guidance and examples continue to use the
shipping APIs. Stage 1 remains open and no later roadmap stage has started.

### Stage 6: initial inconclusive result and approved continuation

The approved evaluation completed smoke in Node/Chromium, then stopped when
the Node empty-control calibration could not meet the approved 20 ms boundary
within 10,000 actions. No correctness failure was observed in the exercised
probes. There is insufficient evidence to adopt or reject the API on cost.
Retain the unchanged shipping implementation and the local prototype/results;
no shipping API, docs promise, staged change or commit is introduced by this
evaluation.

The concrete next decision is the follow-up calibration proposal under Method:
allow up to 100,000 actions for cheap controls and C <= 1 membership cells,
keeping preparation limits, timing minimum, all gates and wall-time ceilings.
The owner approved that amendment on 2026-10-26; continuation is in progress
under the unchanged time ceilings. Complete calibration and the
remaining lab before considering adoption. Complete this stage, including
shipping-path confirmation if later adopted, before starting incremental
aggregates or any later roadmap stage.

### Initial comparison and earlier decisions

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

| Condition                                                             | Result                                                                        |
| --------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| 1. Label update faster at 1,000 items, at most 25% slower at 10       | Holds: −98% at 1,000, −30% at 10                                              |
| 2. Bulk update of every item at most twice the shared map             | Holds: +1%, −12%, +4%                                                         |
| 3. Mount, insertion, removal, reorder and teardown at most 25% slower | Fails for teardown, +30% to +33% at every size; the others hold, at most +12% |
| 4. Memory per item at most 50% higher at 100 and 1,000                | Fails: +84% at both                                                           |
| 5. Done toggle at most 25% slower                                     | Holds: faster at every size                                                   |
| 6. Every correctness check                                            | Holds                                                                         |

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

| Condition                                                                                            | Result                                                                       |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| 1. Label update at 1,000 items meaningfully faster than the shared map, within 2× of store per item  | Holds: 4.92 µs against 339; 0.94× store per item                             |
| 2. Bulk update of every item within +25% of the shared map, at most one derivation and summary write | Holds: label +7%, −1%, −6%; done +8%, −2%, −8%; one derivation and one write |
| 3. Mount, insertion, removal, reorder and teardown within +25%                                       | Holds: at most +2% (mount at 10 items); the others are faster or equal       |
| 4. Heap per item within +25% at 100 and 1,000 items                                                  | Holds: +10% and +11%                                                         |
| 5. Every correctness check, including the prototype tests                                            | Holds                                                                        |

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

**Stage 5 against the thresholds**, for the shipping collection:

| Condition                                                                                           | First timing set                                           | Confirmation                                     |
| --------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- | ------------------------------------------------ |
| 1. Label update at 1,000 items meaningfully faster than the shared map, within 2× of store per item | Holds: 4.96 µs against 341; 0.93× store per item           | Holds: 4.89 µs against 342                       |
| 2. Bulk update of every item within +25%, at most one derivation and summary write                  | Holds: label +6%, −4%, −5%; done +10%, +1%, −11%; one each | Holds: label +12%, −2%, −8%; done +13%, −1%, −9% |
| 3. Mount, insertion, removal, reorder and teardown within +25%                                      | Holds: at most +6%                                         | Holds: at most +7%                               |
| 4. Heap per item within +25% at 100 and 1,000 items                                                 | Holds: +18% and +11%                                       |                                                  |
| 5. Every correctness check                                                                          | Holds, with the package tests                              |                                                  |

**Outcome: confirmed.** The shipping collection meets every threshold in both
timing sets and in the memory and counted sets, and matches the prototype
within run-to-run variation. Bulk allocation, accepted as a cost of this
design, is unchanged from the prototype at about 170 bytes per changed item; in
the traced runs it cost at most one minor collection of about 0.4 ms per
3.4 MB of bulk updates, with no major collections, while single-item updates
triggered fewer collections than the shared map's. These are measurements of
the shipping implementation in headless Chromium on one host, at up to 1,000
items; they do not establish behavior at larger sizes or in other hosts.
