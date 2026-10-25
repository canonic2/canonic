# Selection and windowing at 10,000 items

## Question

At 10,000 items, how should a keyed collection's single selection be held,
and does mounting only the visible rows keep mounting, memory, scrolling and
updates within budget? The answers decide whether the
[collections contract](../specs/contracts/collections.md) recommends a
selection pattern on the existing API or needs a keyed subscription API, and
whether Lattice documents a windowing pattern or proposes a windowing helper.

## Status

Running, agreed by the owner on 2026-10-25 with the amendments below, before
any run. Both stages are authorized. This compares application patterns using
existing APIs; no public API or windowing helper is authorized.

## Hypothesis

- **Selection.** With the selected key in a store field and every row watching
  whether it is the selected one, changing the selection evaluates one
  subscription per row: about 10,000 `read`s for two DOM changes, the same
  shape as the shared map's single-item updates in the collection record.
  Holding selection as an item field, changed in one batch, evaluates two rows'
  subscriptions; a single subscription that assigns the two affected rows also
  evaluates once. Both are expected to cost the same at 1,000 and 10,000 items.
- **Windowing.** Mounting every row costs about 23 µs and 900 bytes per item
  on the shipping collection at 1,000 items, so 10,000 rows are expected to
  take about 230 ms of script time before layout and about 9 MB of heap, plus
  their DOM. Mounting only the rows in view, about 40 plus overscan, is
  expected to make mounting and memory independent of the collection's size;
  scrolling then pays for creating or rebinding the rows that enter.

## Method

Lab code only; no Lattice API is added. Every variant uses the shipping
collection from `@canonic2/lattice/collection`, the example's
`<acme-filter-field>` rows with a `selected` input added in the lab, the same
reconciler, per-row lifetimes and a summary of the open count, as in the
collection record.

### Stage A: selection, every row mounted

| Variant | Selection state | Row wiring |
| --- | --- | --- |
| S0. Scalar selection | `selectedKey` in a store beside the collection | Each row subscribes to a store selection `selectedKey === key`; one subscription per row on one field |
| S1. Item field | Item `selected` fields are the sole selection owner; a private cached key is derived from successful actions, never independently writable. `select(key)` clears the previous item and sets the new one in one collection batch | Each row watches its item's `selected` with `watchItem` |
| S2. List input | `selectedKey` in a store, as S0 | One subscription assigns the list element's `selectedKey` input; its setter updates the old and new retained rows |

S2 preserves state → binding → element separation and needs no binding
exception. A binding that directly addresses individual rows would instead be
a contract alternative requiring review before adoption; it is not this S2.
S1 and S2 receive no preference based on today's contract wording. Setup,
memory and teardown are compared alongside selection changes.

**Workloads**, at 1,000 and 10,000 items: select a seeded random row; move the
selection to the next row 100 times; select the row that is then removed;
select a row, then update an unrelated item.

### Stage B: windowing

| Variant | Rows |
| --- | --- |
| W0. All rows | Every item has a mounted row, as in the collection record |
| W1. Window, create and destroy | A lab window binding watches the keys and a scroll position. With a fixed row height and an overscan of 10 rows on each side, it mounts rows only for the keys in view; rows that leave end their lifetime and are removed, rows that enter are created and bound |
| W2. Window, recycled rows | As W1, but a row that leaves is unbound and kept in a pool, and an entering key rebinds a pooled row with a new lifetime instead of creating one |

All variants use Stage A's leading eligible pattern, recorded in a dated
Method entry before Stage B begins. Selection-change CPU time decides between
eligible patterns when meaningfully different; otherwise setup, total heap
and teardown determine whether one dominates, or the result stays tied. The
scroll container sets its scrollable height from the item count; rows are
positioned by their index. Scope is fixed-height rows at 1,000 and 10,000
items only, with 40 visible rows and 10 overscan rows on each side. No
100,000-item or variable-height expansion is included.

**Workloads**, at 1,000 and 10,000 items: mount to the first presented frame;
teardown; scroll by one row, by one viewport and to the end, each driven by
`scrollTop` from an animation frame and run for 120 frames; a single label
update of a visible item and of an item out of view; a bulk done update of
every item; insertion and removal above the visible rows; selecting a row,
scrolling it out of view and back.

### Measures

- **CPU time** per operation, as `performance.now()` around the synchronous
  operation, as in the collection record. Tiny updates are batched into timed
  loops of at least 1,000 operations, normalized per operation; five warmup
  rounds and ten measured rounds per page, retaining round samples.
- **Work counts:** `read`s, row input assignments, rows created, rebound and
  removed, and summary writes per operation.
- **Frames.** For scrolling: script time per frame from a Chromium trace, the
  number of frames over 16.7 ms, and long tasks over 50 ms.
- **Presented latency.** For a real click selecting a row, through Playwright,
  the Event Timing entry's processing end and duration, as
  [performance and evidence](../specs/contracts/performance.md#rendering-and-responsiveness)
  requires; reported as headless Chromium figures.
  “To screen” means the event's presentation endpoint from Event Timing
  (`startTime + duration`) or an identified presented frame in a trace. Mount
  is triggered by a real input to obtain the same boundary. A frame callback
  is only a paint-opportunity proxy, never proof of presentation. Missing
  presentation evidence leaves the presentation threshold unestablished.
- **Memory:** JavaScript heap and DOM node count after garbage collection,
  mounted minus empty, at each size. Report total heap including collection
  state, and presentation-only heap (mounted minus state-only), separately;
  threshold 6 uses total heap.
- **Allocations and garbage collection** per selection change and per scrolled
  frame, from the sampling heap profiler and the GC trace as in stage 5 of the
  collection record.

### Correctness checks

Every counted page, every variant: the visible rows match the bench's model,
in order, with their current labels, done and selected states; exactly one row
shows as selected, or none after the selected item is removed; an update to an
item out of view makes no DOM write and shows its new state when scrolled into
view; selection survives scrolling the selected row out and back; rows not
entering or leaving keep their elements across updates; teardown leaves no
row, subscription, listener or node. Same-selection actions make no writes;
key reuse creates a new incarnation with current values and no stale callbacks;
selecting an offscreen item persists and is shown when mounted. A focused row
that leaves the window stays mounted, bounded to one extra row, and is never
recycled onto another item. Keyboard navigation mounts its destination before
focusing it. Removing a focused item focuses the next item at the removed
index, or the preceding item at the end, or the list when empty. Insertion and
removal above the viewport preserve the first visible key and its pixel
offset through explicit scroll anchoring. Focus preservation is required.

### Runs

As in the collection record: timing 15 pages per variant and size, memory and
allocation 10, counted 3, presented latency 10 pages of 20 clicks, scrolling
10 pages. Pages alternate variants in a seeded order; each measured operation
has warmup repetitions.

### Agreed thresholds (2026-10-25)

Thresholds apply to candidates; controls need not pass them. A meaningful
difference exceeds 5% of the control and both interquartile ranges. This is
also the meaning of “meaningfully lower than S0”. Confirmation repeats the
control and leading candidate's deciding timing cells under comparable
conditions before reporting a supported result.

**Selection (stage A), at 10,000 items with every row mounted.** A pattern is
supported when:

1. A selection change evaluates at most four row subscriptions and assigns
   only the two affected rows;
2. its CPU time at 10,000 items is within 1.5× of its time at 1,000 items, and
   meaningfully lower than S0's;
3. a clicked selection's presented latency is at most 100 ms at the 95th
   percentile;
4. every correctness check passes.

If neither S1 nor S2 holds, report the unmet thresholds for owner review;
agreement to the lab does not authorize a new API.

**Windowing (stage B), at 10,000 items.** A windowed variant is supported
when:

5. mounting to the first presented frame takes at most 10% of W0's CPU time
   and at most 100 ms presented;
6. JavaScript heap and DOM nodes for the mounted list are at most 25% of
   W0's;
7. while scrolling by one viewport per frame, script time per frame is at most
   4 ms at the 95th percentile, with no long task over 50 ms;
8. an update to an item out of view makes no DOM write, and a visible update
   costs within 25% of W0's;
9. every correctness check passes.

If W1 and W2 both hold, the simpler W1 is preferred unless W2 is meaningfully
cheaper while scrolling. A supported result leads to a documented pattern; a
windowing helper in Lattice is proposed separately, as a new API.

### Host and tools

As the collection record: the same machine, Node, Playwright and headless
Chromium. Presented latency and frame timings in headless Chromium are
reported as such; they do not establish timings on a display.

### Runner reliability amendment (2026-10-25, smoke only)

The first smoke exposed two runner defects: an assignment-count assertion ran
on pages with counters disabled, and the visible-update loop read `scrollTop`
after each write, forcing layout inside the timed boundary. Count assertions
now run only on counted pages, and the visible key is resolved before timing.
The first smoke is diagnostic only; a complete smoke repeats on the corrected
runner before measured sets. No threshold changes. Explicit model checks run
after measurement on every page; resource-count assertions run on counted
pages. The singleton removal check covers fallback to the empty list.

All inputs are generated locally. Browser requests are fulfilled from local
fixture and shipping source; no external calls, network installs or paid
services are needed. Each run owns a fresh results directory and browser
contexts; cancellation closes the browser and retains partial results.

### Stage A confirmation (2026-10-25, before confirmation)

S2 is the CPU leader, meaningfully cheaper than S1, with lower total heap.
Neither candidate clears the first set's presented-latency threshold.
Confirmation repeats S0 and S2 at both sizes: 15 timing pages for random and
next-row selection, and 10 pages of 20 real clicks. Setup and teardown are
not deciding selection thresholds and are not repeated. The original failing
presentation samples remain part of the reported outcome.

### Stage B choice and boundaries (2026-10-25, before any Stage B run)

Neither Stage A candidate is eligible against every all-mounted threshold:
both fail presented latency. To complete the authorized windowing comparison,
Stage B uses S2 provisionally: it leads CPU cost and heap, and its CPU scaling
holds in confirmation. This is an execution choice, not acceptance of a
selection pattern or a change to any threshold.

W0 leaves ordinary scrolling to the browser; it does not reconcile all rows
on scroll. W1 and W2 reconcile the window in the driven animation-frame
callback and respond to native scroll events. A focused input inside a row
is included in the pinning check. The zero-write offscreen workload targets
an unmounted item; a pinned focused row retains its binding and its costs.

Scrolling traces bracket exactly 120 driven frames with timing marks. Script
time is the union of callback, event, function and microtask intervals on the
page's renderer thread between adjacent frame starts, clipped to the marked
interval. Long tasks are recorded through both the Long Tasks observer and
renderer task events. Animation-frame intervals are paint-opportunity cadence,
not presented-frame latency. These boundaries are established before the
windowing smoke; they do not alter the 4 ms or 50 ms limits.

## Conditions

### Stage A (2026-10-25)

| Item | Value |
| --- | --- |
| Time | 03:56–04:37 UTC |
| Host | macOS 15, x86-64, Intel Xeon W-2140B, 16 threads, 64 GB memory; Node 24.12.0; headless Chromium 156.0.8078.4 with Playwright 1.64.1 |
| Load | One-minute load 16.3 before and 8.0 after; an intermediate observation was 25.6. Power state was not sampled |
| Source | Public commit `bd61a1e`; shipping collection SHA-256 `a597d35a…`, store `9bf3b436…`, reporter `e4ad2ac9…`; lab page `659b1be1…`, runner `0ecb66bb…` |
| Samples | Per pattern and size: 15 timing pages, 10 memory/allocation pages, 3 counted pages, 10 presentation pages of 20 clicks; 228 pages total |
| Warm state | CPU setup and teardown: five warmups, ten measured rounds; changing updates: five warmup and ten measured rounds of 1,000 calls. Presented mount is the first construction in a fresh context; imports precede the input event |
| Memory | After forced garbage collection; total is mounted minus empty, presentation is mounted minus state-only. Native DOM bytes are not measured; DOM nodes are counted separately |

The common item record includes a `selected` boolean in all patterns; it is
dormant in S0 and S2. This isolates subscription and wiring differences but
does not attribute S1's extra item-field cost. Allocation sampling includes
the common target-array preparation around the selection loop; CPU update
timers exclude that preparation.

### Stage A confirmation (2026-10-25)

04:38–04:50 UTC, the same host and shipping sources, with lab page
`659b1be1…` and runner `1f28349e…`. One-minute load was 38.4 before and 21.5
after. Per pattern and size: 15 timing pages, five warmup and ten measured
rounds of 1,000 calls; 10 presentation pages of 20 clicks. Setup and heap were
not repeated. The higher load limits comparisons of presentation tails.

## Results

### Stage A (2026-10-25)

CPU figures are p50 / p95 / maximum (IQR) across 15 page medians. Each page's
median is over ten measured rounds. Times exclude style, layout and paint
that happen after the synchronous operation returns.

| Pattern | Items | Mount, ms | Teardown, ms | Random selection, µs | Next selection, µs | Same selection, µs | Unrelated label update, µs |
| --- | --- | --- | --- | --- | --- | --- | --- |
| S0 | 1,000 | 31.6 / 32.1 / 32.1 (0.525) | 14.8 / 15.3 / 15.3 (0.370) | 18.2 / 19.0 / 19.0 (0.430) | 18.4 / 19.0 / 19.0 (0.670) | 0.045 / 0.050 / 0.050 (0.005) | 2.88 / 2.98 / 2.98 (0.080) |
| S0 | 10,000 | 941 / 986 / 986 (41.1) | 172 / 197 / 197 (10.4) | 243 / 577 / 577 (114) | 234 / 349 / 349 (97.5) | 0.050 / 0.050 / 0.050 (0.005) | 2.91 / 3.08 / 3.08 (0.150) |
| S1 | 1,000 | 31.3 / 32.3 / 32.3 (0.945) | 14.9 / 15.6 / 15.6 (0.575) | 2.62 / 2.81 / 2.81 (0.180) | 2.11 / 2.20 / 2.20 (0.120) | 0.045 / 0.055 / 0.055 (0.005) | 3.06 / 3.24 / 3.24 (0.065) |
| S1 | 10,000 | 950 / 1032 / 1032 (27.9) | 168 / 175 / 175 (8.57) | 3.55 / 3.75 / 3.75 (0.115) | 2.38 / 2.68 / 2.68 (0.210) | 0.040 / 0.040 / 0.040 (0.000) | 3.03 / 3.21 / 3.21 (0.110) |
| S2 | 1,000 | 30.8 / 33.0 / 33.0 (1.04) | 13.2 / 14.0 / 14.0 (0.435) | 1.24 / 1.53 / 1.53 (0.100) | 0.995 / 1.38 / 1.38 (0.190) | 0.050 / 0.050 / 0.050 (0.010) | 2.99 / 3.56 / 3.56 (0.150) |
| S2 | 10,000 | 931 / 1021 / 1021 (46.9) | 171 / 177 / 177 (6.58) | 1.70 / 2.06 / 2.06 (0.210) | 1.35 / 1.58 / 1.58 (0.220) | 0.050 / 0.050 / 0.050 (0.005) | 2.90 / 3.05 / 3.05 (0.110) |

Heap and allocation figures use the same p50 / p95 / maximum (IQR) notation
over 10 pages; MB is decimal. Every pattern had 10,005 DOM nodes at 1,000
items and 100,005 at 10,000 items, with no variation.

| Pattern | Items | Total heap, MB | Presentation-only heap, MB | Allocated bytes/selection | Minor GCs/20,000 selections | GC ms/20,000 selections |
| --- | --- | --- | --- | --- | --- | --- |
| S0 | 1,000 | 1.10 / 1.10 / 1.10 (0.000388) | 0.965 / 0.966 / 0.966 (0.000388) | 99.2 / 99.7 / 99.7 (0.414) | 1 / 2 / 2 (1) | 0.575 / 0.961 / 0.961 (0.351) |
| S0 | 10,000 | 10.8 / 10.9 / 10.9 (0.000700) | 9.35 / 9.40 / 9.40 (0.000700) | 99.2 / 99.7 / 99.7 (0.724) | 2 / 2 / 2 (0) | 1.02 / 1.07 / 1.07 (0.079) |
| S1 | 1,000 | 1.04 / 1.04 / 1.04 (0.00123) | 0.900 / 0.902 / 0.902 (0.00123) | 591 / 593 / 593 (0.656) | 5 / 5 / 5 (0) | 1.87 / 1.97 / 1.97 (0.027) |
| S1 | 10,000 | 10.6 / 10.6 / 10.6 (0.0635) | 9.11 / 9.11 / 9.11 (0.0635) | 591 / 593 / 593 (0.486) | 2 / 2 / 2 (0) | 1.39 / 1.50 / 1.50 (0.080) |
| S2 | 1,000 | 0.914 / 0.914 / 0.914 (0.00114) | 0.777 / 0.778 / 0.778 (0.00114) | 100 / 100 / 100 (0.330) | 2 / 2 / 2 (0) | 0.905 / 1.02 / 1.02 (0.068) |
| S2 | 10,000 | 9.11 / 9.11 / 9.11 (0.00213) | 7.65 / 7.65 / 7.65 (0.00213) | 99.2 / 100 / 100 (0.582) | 0 / 0 / 0 (0) | 0 / 0 / 0 (0) |

No traced selection run had a major GC. S1's two item-record changes and
batch bookkeeping allocate substantially more than S2's scalar write; these
are whole-pattern allocation measurements, including the common runner cost.

Event Timing, p50 / p95 / maximum (IQR), in milliseconds. Processing end is
relative to event start; duration is the headless presentation measurement.
There are ten mount events and up to 200 trusted click entries per cell.
Synthetic label-generated clicks with interaction ID zero are excluded.

| Pattern | Items | Mount presented | Click presented | Click processing end | Observed click entries |
| --- | --- | --- | --- | --- | --- |
| S0 | 1,000 | 184 / 232 / 232 (0) | 16 / 16 / 16 (0) | 6.36 / 8.50 / 11.2 (1.04) | 200/200 |
| S0 | 10,000 | 1800 / 1840 / 1840 (32) | 96 / 120 / 128 (8) | 72.5 / 92.7 / 99.3 (6.58) | 200/200 |
| S1 | 1,000 | 184 / 184 / 184 (0) | 16 / 16 / 16 (0) | 5.94 / 8.02 / 11.0 (0.965) | 199/200 |
| S1 | 10,000 | 1784 / 1808 / 1808 (32) | 96 / 112 / 120 (8) | 71.7 / 80.9 / 97.8 (6.29) | 200/200 |
| S2 | 1,000 | 184 / 200 / 200 (0) | 16 / 16 / 16 (0) | 6.17 / 8.06 / 10.6 (1.09) | 198/200 |
| S2 | 10,000 | 1784 / 8408 / 8408 (40) | 96 / 120 / 528 (8) | 70.9 / 95.8 / 447 (5.93) | 200/200 |

No value is imputed for missing Event Timing entries. The 10,000-item cells
have complete click coverage. One S2 page had a much slower mount and clicks;
it remains in the percentiles and maxima.

Observed selection-change counts, identical on all three counted pages at
each size:

| Pattern | Selection reads at 1,000 / 10,000 | Row assignments | Summary writes |
| --- | --- | --- | --- |
| S0 | 1,000 / 10,000 | 2 | 0 |
| S1 | 2 / 2 | 2 | 0 |
| S2 | 1 / 1, a list subscription | 2 | 0 |

S2's random selection is 52% cheaper than S1 at 10,000 items, a meaningful
difference. Its total heap is about 1.5 MB smaller. Setup and teardown at
10,000 items are within the observed variation between S1 and S2. S2 is the
CPU and footprint leader; that does not establish the presentation budget.

### Stage A confirmation (2026-10-25)

Same summary notation as the first set; 15 pages per timing cell.

| Pattern | Items | Random selection, µs | Next selection, µs |
| --- | --- | --- | --- |
| S0 | 1,000 | 17.3 / 18.2 / 18.2 (0.520) | 16.7 / 18.7 / 18.7 (0.555) |
| S0 | 10,000 | 196 / 250 / 250 (11.7) | 188 / 210 / 210 (13.6) |
| S2 | 1,000 | 1.20 / 3.46 / 3.46 (0.110) | 1.03 / 2.18 / 2.18 (0.130) |
| S2 | 10,000 | 1.80 / 1.88 / 1.88 (0.045) | 1.45 / 1.53 / 1.53 (0.110) |

Event Timing, ten pages per cell:

| Pattern | Items | Mount presented, ms | Click presented, ms | Click processing end, ms | Observed click entries |
| --- | --- | --- | --- | --- | --- |
| S0 | 1,000 | 192 / 208 / 208 (8) | 16 / 16 / 24 (0) | 6.69 / 9.09 / 11.3 (1.11) | 198/200 |
| S0 | 10,000 | 1840 / 5280 / 5280 (64) | 96 / 432 / 568 (8) | 74.4 / 332 / 495 (6.18) | 200/200 |
| S2 | 1,000 | 192 / 216 / 216 (8) | 16 / 16 / 16 (0) | 6.71 / 8.32 / 10.1 (1.09) | 200/200 |
| S2 | 10,000 | 1816 / 9416 / 9416 (88) | 96 / 160 / 584 (8) | 73.2 / 120 / 490 (8.18) | 200/200 |

S2 remains meaningfully cheaper than S0. CPU growth is 1.494× for random
selection and 1.415× for next-row selection, both within 1.5×, with little
margin for the random case. Presented p95 is 160 ms, failing threshold 3
again. Presentation tails vary substantially between sets; neither set is
discarded. All 100 confirmation pages passed their model checks.

## Correctness

### Smoke (2026-10-25)

The first selection smoke completed 24 cells with two assertion failures from
the counter-mode runner defect above and no page errors. Timing cells from
that diagnostic run are not used as performance evidence. The corrected smoke
completed all 24 cells at both sizes with no failed correctness check or page
error, including counted teardown returning nodes and listeners to baseline.

### Stage A (2026-10-25)

All three patterns passed: visible values and order against the model;
selection changes assigning only the affected rows; same-selection actions
doing no selection read or assignment; unrelated updates retaining row
identity; clearing a removed selection; deterministic focus fallback to the
next, preceding or empty-list target; key reuse across commits and in one
batch creating a fresh row without stale callbacks; and the bulk-update model.
Every measurement page ran model checks after its timed or profiled work.
The six counted pages per pattern additionally returned subscription, DOM
node and listener counts to baseline after teardown. Window-specific focus,
offscreen and anchoring checks belong to Stage B.

## Threats to validity

- Fixed-height example rows at 1,000 and 10,000 items, one engine and one
  shared host. This does not establish windowing for every page-list layout.
- Headless Event Timing measures Chromium's presentation endpoint, not a
  frame observed on a physical display. Entries are quantized; the observed
  96/104 ms boundary is close to the 100 ms budget.
- Twenty clicks share each page and are correlated. Presented percentiles
  pool observed trusted clicks; CPU percentiles summarize page medians.
- Background load varies and was not sampled continuously. The slow S2 page
  is retained; no cause for its outlier is established.
- CPU setup excludes deferred rendering. The input-triggered mount measures
  a separate first-use presentation boundary, including browser work.
- The common dormant item flag prevents attribution of S1's item-field
  footprint. Heap excludes native DOM bytes; profiler allocation includes
  common target preparation and varies with sampling and GC behavior.

## Decision and follow-ups

### Stage A against its thresholds (2026-10-25)

| Threshold | S1 | S2 |
| --- | --- | --- |
| 1. At most four row reads, only two row assignments | Holds: two reads, two assignments | Holds: one list read, two assignments |
| 2. At most 1.5× CPU growth and meaningfully lower than S0 | Holds: random 1.36×, next 1.13× | Holds: random 1.38×, next 1.35× |
| 3. Click presented p95 at most 100 ms | Fails: 112 ms | Fails: 120 ms |
| 4. Correctness | Holds | Holds |

Neither candidate is supported against every all-mounted threshold. S2 leads
synchronous selection CPU time and footprint without a binding-contract
exception. Its CPU result holds in confirmation, while presented p95 fails
again at 160 ms. Stage B uses S2 provisionally as recorded in Method; no
product pattern, helper or API is adopted by these measurements. The owner
decides on the complete reported evidence.
