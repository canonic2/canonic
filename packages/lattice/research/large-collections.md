# Selection and windowing at 10,000 items

## Question

At 10,000 items, how should a keyed collection's single selection be held,
and does mounting only the visible rows keep mounting, memory, scrolling and
updates within budget? The answers decide whether the
[collections contract](../specs/contracts/collections.md) recommends a
selection pattern on the existing API or needs a keyed subscription API, and
whether Lattice documents a windowing pattern or proposes a windowing helper.

## Status

Running, 2026-10-25: the owner authorized the W1 profiling follow-up. The
owner agreed the original plan with the four requested
amendments on 2026-10-25, before the first run. Dated execution changes are
recorded below. Both stages, confirmation and secondary timing coverage are
complete. This compares application patterns using existing APIs; the owner's
adoption decision is pending. No public API or windowing helper is authorized.

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

### Stage B instrumentation amendment (2026-10-25, after smoke)

The first windowing smoke passed every correctness check but review found the
heap sampling profiler active during the frame-timing trace. That couples
allocation instrumentation to the CPU and GC result. The full runner uses
separate passes: 120 frames with tracing only for CPU, cadence, long tasks
and GC; then a separate 120-frame allocation-sampling pass. Both start from
scroll position zero, outside measurement, after warmup. The first smoke's
profiled frame times are diagnostic only; a complete smoke repeats before
the full set. No candidate code or threshold changes.

Presented mount latency is unaffected by that defect: both 10,000-item
windowed smoke cells measured 112 ms, above 100 ms. That is a preliminary
finding, not a runner failure, and is retained for the full comparison.

### Focused key-reuse amendment (2026-10-25, before the full Stage B set)

Review after the smoke found a candidate correctness gap in the shared list:
focus fallback checked whether the key remained in membership, which misses
removal and reinsertion of the same focused key in one batch. The list now
checks the removed-incarnation change as well and focuses the item at the
removed index, including a replacement incarnation there. Coverage combines
focus and batched key reuse, rather than checking them only separately.
Focused counted pages repeat all three selection patterns at both sizes;
then the complete windowing smoke repeats. Selection hot paths and thresholds
are unchanged. Earlier correctness reports cover their listed cases, not this
additional combination.

For the windowing comparison, counted pages also capture mount, a steady
one-row step, a viewport step, an end jump, visible and unmounted label
updates, bulk done changes, and insertion/removal above the viewport.
Counters distinguish row construction, rebinding, removal, input assignment,
position writes and summary writes. Meaningful scrolling comparisons use
per-page medians of traced script cost; the absolute frame budget uses the
pooled frame p95, with the page count stated.

The row-rebinding review also found that a recycled search field needs its
existing `value` input reset for the next item. Fresh and rebound rows receive
the empty value; retained rows do not. The pinning check now keeps a draft in
the focused control, verifies that it survives scrolling, and checks that it
does not appear in a different recycled item. This is a candidate correction
before the full Stage B set, not a new framework API. Summary derivations are
counted separately, with one derivation and one write for a bulk done change.

### Secondary timing coverage amendment (2026-10-25, during Stage B)

Review of the runner found that insertion and removal above the viewport had
work-count and anchoring coverage but no CPU timing cell. The active set
stays unchanged. A follow-up completes those planned secondary timings for
W0, W1 and W2 at both sizes, 15 pages each: five warmup and ten measured
rounds of 20 alternating insertion/removal pairs above a viewport starting
at row 100. Each action is timed synchronously and normalized per action;
the burst includes layout forced by the next action's anchor read. The
viewport is prepared outside the timers. These are burst CPU figures, not
single-input presentation latency.

The same pages time selecting alternating unmounted end keys, five warmup
and ten measured rounds of 1,000 changes, with W0 retaining those rows as its
control. Model checks run after measurement. This completes the existing
workloads without changing the primary metrics, thresholds or candidates.

### Stage B confirmation coverage (2026-10-25, before confirmation)

Confirmation repeats W0, W1 and W2 at 10,000 items, where thresholds 5–9
decide support: 15 CPU timing pages for mount and visible updates, 10
presentation pages, and 10 scrolling pages for the one-viewport-per-frame
case. Both windowed variants repeat because their relative scrolling cost
also informs the choice. The 1,000-item context, row-step and end-jump cells
remain in the first set and are not deciding confirmation cells. Allocation
sampling and heap are not repeated; confirmation scrolling uses tracing
only, matching the first set's timing pass.

### W1 profiling follow-up (2026-10-25, before profiling)

The owner authorizes profiling W1's missed budgets, proposing targeted changes
from that evidence and comparing them with unchanged W1. Stages A and B remain
reported. This follow-up is running; no public API or helper is authorized.

Freeze the current W1 application source as the control. Use the same S2 input,
24 px rows, 960 px viewport, overscan, seeds and 1,000/10,000-item fixtures.
The hypothesis is that repeated reconciliation for an unchanged range adds
avoidable work, while entering-row binding and DOM lifecycle still dominate
large window steps. This is a hypothesis, not an attribution of prior tails.

First profile unchanged W1. Diagnostic instrumentation is separate from
comparison timings. Record mount click Event Timing input delay
(`processingStart - startTime`), event-handler interval
(`processingEnd - processingStart`), and the residual to its presentation
endpoint (`startTime + duration - processingEnd`). The residual includes
scheduling and presentation wait; it is not a rendering CPU measurement.
Correlate trace style, layout, paint and GC work with those boundaries. An
animation-frame callback remains a callback-cadence proxy only.

For scrolling, retain the row, viewport and end-jump inputs. Instrument render
entries by caller, unchanged ranges, reconciliation, row construction, binding
setup/disposal and DOM mutation. Attribute sampled JavaScript allocations by
stack, with native DOM memory outside that sampling measure. Keep GC trace
time separate, avoid summing overlapping parent/child intervals, and report
unattributed work. CPU profiles and diagnostic spans explain costs; they do
not replace the uninstrumented frame-script metric.

Smoke each diagnostic cell once, then take three fresh diagnostic pages per
size. Record the evidence and candidate changes before candidate runs. Compare
unchanged W1 and each targeted candidate in seeded order: 15 timing pages,
10 presentation pages, 10 scrolling pages and 10 heap pages per size, with
the existing warmups and 120-frame sequences. Retain W0 timing/heap reference
cells for the relative budgets. Confirm the leading candidate against W1 at
10,000 items with 15 timing, 10 presentation and 10 scrolling pages. Smoke
every comparison group with one page per cell first.

Budgets 5–9 and the meaningful-difference rule are unchanged. Same-selection
no-ops, offscreen selection, focused-row pinning (one extra row), keyboard
destinations, deterministic removal fallback, insertion/removal anchoring,
new incarnations on key reuse, model output and teardown remain required.
Measure total and presentation-only heap separately. A candidate that misses
a budget remains a miss even if it improves. Report all variants and failed
or partial runs. Existing local Chromium tooling and read-only shipping
modules are used; no external data or network transmission is needed.

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

Fixtures use keys `t0` through `t(N−1)`, labels `Task i`, `done` when
`i % 3 === 0`, and `selected: false` initially. Rows are 24 CSS pixels high
in a 960-pixel viewport. The open-count summary is a native output whose
text is assigned by one collection-wide subscription. Target generation and
job shuffling start with seed 1 and the unsigned 32-bit recurrence
`s = 1664525 × s + 1013904223`; random targets use `s % N`, and the
Fisher–Yates shuffle uses `s % (index + 1)`.

### Stage A confirmation (2026-10-25)

04:38–04:50 UTC, the same host and shipping sources, with lab page
`659b1be1…` and runner `1f28349e…`. One-minute load was 38.4 before and 21.5
after. Per pattern and size: 15 timing pages, five warmup and ten measured
rounds of 1,000 calls; 10 presentation pages of 20 clicks. Setup and heap were
not repeated. The higher load limits comparisons of presentation tails.

### Stage B (2026-10-25)

| Item | Value |
| --- | --- |
| Time | 05:10–05:49 UTC |
| Host and tools | As Stage A; one owned browser per set, fresh context per page; browser-level caches may stay warm |
| Load | One-minute load 11.2 before and 6.5 after; not sampled continuously |
| Source | Public commit `bae472c`; shipping source hashes unchanged from Stage A; corrected lab page `443c7dc0…`, runner `d3dad200…` |
| Samples | Per window and size: 15 timing pages, 10 heap/allocation pages, 3 counted pages, 10 presentation pages of 20 clicks, 10 scrolling pages; 288 pages total |
| Scrolling | Five categories of work are separate: driven synchronous samples, traced script intervals, callback cadence, long tasks, and sampled allocations. Timing/GC uses a trace-only pass; allocations use a later sampling-only pass |
| Sampling | 32-byte interval for selection allocation; 32,768-byte interval for scrolling allocation, including objects collected during sampling |
| Memory | Initial 50-row window, no focused pin or accumulated recycle pool; mounted minus empty and mounted minus state-only after forced GC |

### Stage B confirmation (2026-10-25)

05:50–06:03 UTC, same host and shipping hashes, lab page `443c7dc0…` and
runner `05f4bf9b…`. One-minute load was 5.0 before and 13.9 after. At 10,000
items, each window has 15 CPU pages, 10 presentation pages of 20 clicks and
10 viewport-scrolling pages, 105 pages total. The page code is identical to
the first set; only the runner's confirmation coverage changes. Allocation
sampling is absent from confirmation; its scrolling timing pass is traced
as in the first set.

### Secondary timing coverage (2026-10-25)

06:05–06:23 UTC, same host and shipping hashes. The lab page `c6711749…`
adds measurement entry points to the unchanged candidate code; runner
`ebb4f265…` selects the secondary cells. One-minute load was 7.7 before and
11.8 after. The six-cell smoke passed, then 15 fresh pages per window and
size ran five warmup and ten measured rounds: 20 insertion/removal pairs
and 1,000 offscreen selection changes per round, 90 pages total.

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

### Stage B (2026-10-25)

All variants use S2. CPU figures are p50 / p95 / maximum (IQR) over 15 page
medians; heap and allocation figures use 10 pages. Bulk done updates use two
calls per round; tiny label updates use 1,000. No frame profiler is active
during these CPU timings.

| Window | Items | Mount, ms | Teardown, ms | Visible label, µs | Offscreen label, µs | Bulk done, ms |
| --- | --- | --- | --- | --- | --- | --- |
| W0 | 1,000 | 31.0 / 38.8 / 38.8 (1.82) | 13.3 / 15.2 / 15.2 (0.730) | 2.89 / 3.60 / 3.60 (0.105) | 2.94 / 3.45 / 3.45 (0.095) | 3.54 / 4.47 / 4.47 (0.265) |
| W0 | 10,000 | 944 / 1170 / 1170 (49.2) | 172 / 178 / 178 (8.21) | 2.90 / 3.12 / 3.12 (0.120) | 2.88 / 3.10 / 3.10 (0.190) | 43.1 / 146 / 146 (3.01) |
| W1 | 1,000 | 2.61 / 2.78 / 2.78 (0.200) | 0.870 / 0.975 / 0.975 (0.065) | 3.04 / 3.31 / 3.31 (0.155) | 0.300 / 0.320 / 0.320 (0.005) | 0.485 / 0.535 / 0.535 (0.030) |
| W1 | 10,000 | 7.47 / 8.01 / 8.01 (0.310) | 0.875 / 0.950 / 0.950 (0.055) | 2.94 / 3.45 / 3.45 (0.080) | 0.325 / 0.350 / 0.350 (0.005) | 3.44 / 3.59 / 3.59 (0.135) |
| W2 | 1,000 | 2.55 / 4.31 / 4.31 (0.180) | 0.880 / 1.53 / 1.53 (0.055) | 3.06 / 5.25 / 5.25 (0.240) | 0.305 / 0.355 / 0.355 (0.005) | 0.493 / 0.905 / 0.905 (0.035) |
| W2 | 10,000 | 7.34 / 8.12 / 8.12 (0.370) | 0.890 / 0.955 / 0.955 (0.040) | 2.92 / 3.94 / 3.94 (0.090) | 0.330 / 0.350 / 0.350 (0.005) | 3.54 / 3.80 / 3.80 (0.190) |

Both windows mount in less than 1% of W0's synchronous time at 10,000
items, and visible label updates are within 2% of W0. Bulk changes retain
the collection's linear state and aggregate work while assigning only mounted
rows.

| Window | Items | Total heap, MB | Presentation-only heap, MB | DOM nodes | Allocated bytes/selection | Minor GCs/20,000 selections | GC ms/20,000 selections |
| --- | --- | --- | --- | --- | --- | --- | --- |
| W0 | 1,000 | 0.914 / 0.915 / 0.915 (0.000780) | 0.777 / 0.778 / 0.778 (0.000780) | 10,005 | 100 / 101 / 101 (0.674) | 2 / 2 / 2 (0) | 0.890 / 0.994 / 0.994 (0.058) |
| W0 | 10,000 | 9.11 / 9.11 / 9.11 (0.00167) | 7.65 / 7.65 / 7.65 (0.00167) | 100,005 | 99.2 / 100 / 100 (0.919) | 0 / 0 / 0 (0) | 0 / 0 / 0 (0) |
| W1 | 1,000 | 0.181 / 0.187 / 0.187 (0.00000) | 0.0438 / 0.0499 / 0.0499 (0.00000) | 505 | 101 / 101 / 101 (0.361) | 1 / 1 / 1 (0) | 0.537 / 0.659 / 0.659 (0.035) |
| W1 | 10,000 | 1.51 / 1.52 / 1.52 (0.00609) | 0.0439 / 0.0500 / 0.0500 (0.00609) | 505 | 101 / 101 / 101 (0.426) | 2 / 2 / 2 (0) | 0.877 / 1.05 / 1.05 (0.096) |
| W2 | 1,000 | 0.181 / 0.187 / 0.187 (0.000100) | 0.0438 / 0.0499 / 0.0499 (0.00000) | 505 | 101 / 101 / 101 (0.549) | 1 / 1 / 1 (1) | 0.531 / 0.690 / 0.690 (0.584) |
| W2 | 10,000 | 1.51 / 1.52 / 1.52 (0.00609) | 0.0439 / 0.0500 / 0.0500 (0.00609) | 505 | 101 / 101 / 101 (0.435) | 2 / 2 / 2 (0) | 0.928 / 1.01 / 1.01 (0.094) |

DOM counts have no variation. Heap is measured at the initial 50-row window,
without a focus pin. At 10,000 items, both windows use 16.6% of W0's total
heap and 0.505% of its DOM nodes. Presentation-only heap stays near 44 KB
at both sizes; total heap still grows with collection state. The hypothesis
of size-independent memory holds for presentation, not for total retained
state. Selection profiles include common runner preparation as in Stage A.

Event Timing over ten fresh pages per cell, in milliseconds:

| Window | Items | Mount presented | Click presented | Click processing end | Observed mount / click entries |
| --- | --- | --- | --- | --- | --- |
| W0 | 1,000 | 184 / 408 / 408 (8) | 16 / 16 / 24 (0) | 6.94 / 8.49 / 11.6 (1.28) | 10/10; 199/200 |
| W0 | 10,000 | 1800 / 2040 / 2040 (200) | 96 / 120 / 384 (8) | 72.3 / 91.7 / 285 (7.97) | 10/10; 200/200 |
| W1 | 1,000 | 88 / 136 / 136 (8) | 16 / 16 / 16 (0) | 1.50 / 2.50 / 2.77 (0.530) | 10/10; 199/200 |
| W1 | 10,000 | 104 / 112 / 112 (8) | 16 / 16 / 16 (0) | 1.48 / 2.42 / 2.66 (0.515) | 10/10; 199/200 |
| W2 | 1,000 | 88 / 144 / 144 (8) | 16 / 16 / 24 (0) | 1.50 / 2.39 / 2.71 (0.515) | 10/10; 199/200 |
| W2 | 10,000 | 104 / 136 / 136 (16) | 16 / 16 / 24 (0) | 1.50 / 2.36 / 2.90 (0.505) | 10/10; 198/200 |

Windowed clicked selections have an observed p95 of 16 ms, but first mount
still exceeds 100 ms presented. CPU mount time alone does not establish
presentation latency. Missing click entries are not imputed.

Scrolling: 10 pages per cell, 120 driven frames per page. Script summaries
pool 1,200 frames; animation-frame interval summaries pool 1,190 intervals.
Intervals describe callback cadence, not presentation. Long-task counts are
from the page's Long Tasks observer; nested trace task records corroborate
them and are not counted as independent tasks.

| Window | Items | Motion | Script ms/frame, p50 / p95 / max (IQR) | Callback interval ms, p50 / p95 / max (IQR) | Intervals >16.7 ms | Tasks >50 ms |
| --- | --- | --- | --- | --- | --- | --- |
| W0 | 1,000 | Row | 0.118 / 0.140 / 0.192 (0.012) | 16.7 / 16.7 / 16.7 (0.005) | 0 | 0 |
| W0 | 1,000 | Viewport | 0.116 / 0.134 / 0.160 (0.012) | 16.7 / 16.7 / 16.7 (0.005) | 0 | 0 |
| W0 | 1,000 | End | 0.112 / 0.131 / 18.1 (0.009) | 16.7 / 16.7 / 117 (0.005) | 5 | 3 |
| W0 | 10,000 | Row | 0.117 / 0.246 / 2.94 (0.014) | 16.7 / 33.3 / 233 (0.005) | 76 | 21 |
| W0 | 10,000 | Viewport | 0.103 / 0.139 / 2.95 (0.011) | 33.3 / 50.0 / 350 (0.005) | 1188 | 56 |
| W0 | 10,000 | End | 0.100 / 0.120 / 0.206 (0.008) | 33.3 / 50.0 / 66.7 (0.005) | 1178 | 17 |
| W1 | 1,000 | Row | 0.728 / 0.799 / 1.00 (0.049) | 16.7 / 16.7 / 16.7 (0.005) | 0 | 0 |
| W1 | 1,000 | Viewport | 3.95 / 4.63 / 5.84 (0.281) | 16.7 / 16.7 / 16.7 (0.005) | 0 | 0 |
| W1 | 1,000 | End | 4.57 / 4.97 / 5.37 (0.200) | 16.7 / 16.7 / 16.7 (0.005) | 0 | 0 |
| W1 | 10,000 | Row | 0.729 / 0.827 / 0.971 (0.060) | 16.7 / 16.7 / 16.7 (0.005) | 0 | 0 |
| W1 | 10,000 | Viewport | 4.01 / 4.46 / 136 (0.223) | 16.7 / 16.7 / 133 (0.005) | 9 | 3 |
| W1 | 10,000 | End | 4.58 / 5.30 / 45.0 (0.219) | 16.7 / 16.7 / 117 (0.005) | 5 | 3 |
| W2 | 1,000 | Row | 0.630 / 0.695 / 0.949 (0.047) | 16.7 / 16.7 / 16.7 (0.005) | 0 | 0 |
| W2 | 1,000 | Viewport | 3.67 / 4.28 / 5.91 (0.237) | 16.7 / 16.7 / 16.7 (0.005) | 0 | 0 |
| W2 | 1,000 | End | 4.12 / 4.56 / 21.4 (0.199) | 16.7 / 16.7 / 100 (0.005) | 1 | 0 |
| W2 | 10,000 | Row | 0.629 / 1.06 / 4.66 (0.050) | 16.7 / 16.7 / 16.7 (0.005) | 0 | 0 |
| W2 | 10,000 | Viewport | 3.75 / 9.76 / 175 (0.255) | 16.7 / 16.7 / 183 (0.005) | 47 | 10 |
| W2 | 10,000 | End | 4.05 / 4.33 / 4.73 (0.151) | 16.7 / 16.7 / 16.7 (0.005) | 0 | 0 |

W1's viewport cell contains three long tasks of 55–133 ms; W2's contains
ten of 51–186 ms. All remain in the results. Per-page median script times
at 10,000 items, with 10 pages per cell:

| Window | Viewport script ms, p50 / p95 / max (IQR) |
| --- | --- |
| W0 | 0.102 / 0.110 / 0.110 (0.004) |
| W1 | 4.01 / 4.24 / 4.24 (0.061) |
| W2 | 3.71 / 9.58 / 9.58 (0.074) |

W2's typical viewport script cost is about 7.5% lower than W1's, a meaningful
difference under the agreed rule, while its first-set tail is worse. Both
miss the 4 ms p95 and zero-long-task conditions. Cheap W0 script callbacks
coexist with slow callback cadence and long tasks; script alone is not a
scrolling responsiveness measure.

Allocation is sampled in a separate pass. Figures below are p50 / p95 /
maximum (IQR) across ten page samples. KB is decimal. GC is the trace-event
total during the 120 timing frames; per-frame GC cost is that total divided
by 120. Counts show total events and major events as separate distributions.

| Window | Items | Motion | Allocated KB/frame | GC events/120 frames | Major events/120 frames | GC ms/120 frames |
| --- | --- | --- | --- | --- | --- | --- |
| W0 | 1,000 | Row | 0.546 / 0.820 / 0.820 (0.282) | 0 / 0 / 0 (0) | 0 / 0 / 0 (0) | 0 / 0 / 0 (0) |
| W0 | 1,000 | Viewport | 0.273 / 0.828 / 0.828 (0.546) | 1 / 1 / 1 (0) | 1 / 1 / 1 (0) | 2.99 / 3.89 / 3.89 (0.516) |
| W0 | 1,000 | End | 0.273 / 0.547 / 0.547 (0.000233) | 0 / 0 / 0 (0) | 0 / 0 / 0 (0) | 0 / 0 / 0 (0) |
| W0 | 10,000 | Row | 0.273 / 0.823 / 0.823 (0.550) | 0 / 0 / 0 (0) | 0 / 0 / 0 (0) | 0 / 0 / 0 (0) |
| W0 | 10,000 | Viewport | 0.548 / 0.820 / 0.820 (0.273) | 0 / 0 / 0 (0) | 0 / 0 / 0 (0) | 0 / 0 / 0 (0) |
| W0 | 10,000 | End | 0.273 / 1.64 / 1.64 (0.548) | 0 / 0 / 0 (0) | 0 / 0 / 0 (0) | 0 / 0 / 0 (0) |
| W1 | 1,000 | Row | 35.4 / 42.2 / 42.2 (5.23) | 3 / 3 / 3 (0) | 0 / 0 / 0 (0) | 2.22 / 2.40 / 2.40 (0.117) |
| W1 | 1,000 | Viewport | 229 / 238 / 238 (10.6) | 12 / 13 / 13 (1) | 8 / 8 / 8 (0) | 24.4 / 24.9 / 24.9 (0.602) |
| W1 | 1,000 | End | 263 / 276 / 276 (11.4) | 12 / 12 / 12 (0) | 12 / 12 / 12 (0) | 22.0 / 23.9 / 23.9 (1.18) |
| W1 | 10,000 | Row | 35.3 / 38.9 / 38.9 (4.39) | 1 / 1 / 1 (0) | 0 / 0 / 0 (0) | 1.14 / 1.31 / 1.31 (0.059) |
| W1 | 10,000 | Viewport | 229 / 245 / 245 (9.90) | 10 / 10 / 10 (0) | 6 / 6 / 6 (0) | 23.2 / 51.3 / 51.3 (0.754) |
| W1 | 10,000 | End | 262 / 281 / 281 (15.7) | 8 / 11 / 11 (0) | 8 / 9 / 9 (0) | 19.2 / 102 / 102 (1.27) |
| W2 | 1,000 | Row | 34.3 / 40.3 / 40.3 (4.12) | 3 / 3 / 3 (0) | 0 / 0 / 0 (0) | 2.18 / 2.38 / 2.38 (0.106) |
| W2 | 1,000 | Viewport | 210 / 232 / 232 (13.8) | 15 / 16 / 16 (2) | 8 / 8 / 8 (0) | 25.0 / 29.6 / 29.6 (2.47) |
| W2 | 1,000 | End | 257 / 275 / 275 (9.80) | 10 / 10 / 10 (0) | 10 / 10 / 10 (0) | 18.2 / 19.9 / 19.9 (0.441) |
| W2 | 10,000 | Row | 35.6 / 40.6 / 40.6 (4.70) | 1 / 1 / 1 (0) | 0 / 0 / 0 (0) | 1.10 / 1.46 / 1.46 (0.072) |
| W2 | 10,000 | Viewport | 215 / 227 / 227 (12.7) | 10 / 10 / 10 (0) | 6 / 6 / 6 (0) | 21.7 / 54.7 / 54.7 (2.41) |
| W2 | 10,000 | End | 260 / 263 / 263 (8.91) | 7 / 10 / 10 (0) | 7 / 7 / 7 (0) | 15.8 / 20.2 / 20.2 (1.45) |

Recycling avoids row construction but still rebinds entering items. In the
viewport case it allocates about 215 KB/frame against W1's 229 KB/frame,
including common driving and instrumentation costs. No allocation threshold
was agreed.

Observed counts are identical on the three counted pages at each size:

Label and done counts are binding assignments, not setter-entry counts;
attribute reflection can re-enter the example's setters. Position counts
are changed `top` assignments.

| Operation | W0 | W1 | W2 |
| --- | --- | --- | --- |
| Initial mount | N rows created and positioned | 50 rows created and positioned | 50 rows created and positioned |
| Steady one-row step | No row work | 1 created, 1 removed | 1 rebound, 1 removed |
| Viewport step | No row work | 40 created, 40 removed | 40 rebound, 40 removed |
| End jump | No row work | 50 created, 60 removed | 50 rebound, 60 removed |
| Visible label change | 1 label assignment | 1 label assignment | 1 label assignment |
| Offscreen label change | 1 label assignment | No row assignment or position write | No row assignment or position write |
| Bulk done at the initial window | N done assignments; 1 derivation and summary write | 50 done assignments; 1 derivation and summary write | 50 done assignments; 1 derivation and summary write |
| Insert above viewport | 1 row created, N+1 position writes; 1 derivation and summary write | No row created; 60 position writes; 1 derivation and summary write | No row created; 60 position writes; 1 derivation and summary write |
| Remove above viewport | 1 row removed, N position writes; 1 derivation and summary write | No row removed; 60 position writes; 1 derivation and summary write | No row removed; 60 position writes; 1 derivation and summary write |

Each entering row receives label, done and selected inputs once; its position
is assigned once. A recycled row is unbound before rebinding. Focus pinning
adds at most one row to the 60-row window; a focused row is never rebound to
another item. These counts do not include a claim of zero work in a pinned
offscreen row whose own item changes.

### Stage B confirmation (2026-10-25)

The candidate page is unchanged from the first set. CPU timings use 15 pages
per window at 10,000 items; presentation and viewport scrolling use 10.

| Window | Mount ms, p50 / p95 / max (IQR) | Teardown ms | Visible label µs |
| --- | --- | --- | --- |
| W0 | 919 / 980 / 980 (47.9) | 165 / 175 / 175 (15.3) | 2.89 / 3.11 / 3.11 (0.080) |
| W1 | 7.31 / 8.00 / 8.00 (0.235) | 0.880 / 0.965 / 0.965 (0.050) | 2.92 / 3.08 / 3.08 (0.055) |
| W2 | 7.18 / 7.62 / 7.62 (0.390) | 0.875 / 0.940 / 0.940 (0.030) | 2.94 / 3.23 / 3.23 (0.155) |

Event Timing, milliseconds, with the same summary notation:

| Window | Mount presented | Click presented | Click processing end | Observed mount / click entries |
| --- | --- | --- | --- | --- |
| W0 | 1776 / 1928 / 1928 (32) | 96 / 120 / 376 (8) | 71.8 / 92.2 / 311 (7.64) | 10/10; 200/200 |
| W1 | 104 / 120 / 120 (8) | 16 / 16 / 24 (0) | 1.46 / 2.44 / 2.79 (0.495) | 10/10; 199/200 |
| W2 | 104 / 216 / 216 (8) | 16 / 16 / 16 (0) | 1.51 / 2.50 / 5.08 (0.500) | 10/10; 196/200 |

Viewport-per-frame scrolling, 1,200 traced frames and 1,190 callback
intervals per window:

| Window | Script ms/frame, p50 / p95 / max (IQR) | Page median script ms | Callback interval ms | Intervals >16.7 ms | Tasks >50 ms | GC ms/120 frames |
| --- | --- | --- | --- | --- | --- | --- |
| W0 | 0.102 / 0.124 / 0.184 (0.009) | 0.101 / 0.104 / 0.104 (0.001) | 33.3 / 50.0 / 100 (0.005) | 1187 | 18 | 0 / 0 / 0 (0) |
| W1 | 4.13 / 4.58 / 5.22 (0.420) | 4.12 / 4.21 / 4.21 (0.066) | 16.7 / 16.7 / 16.7 (0.005) | 0 | 0 | 21.2 / 23.3 / 23.3 (2.54) |
| W2 | 3.85 / 6.31 / 142 (0.371) | 3.80 / 6.31 / 6.31 (0.087) | 16.7 / 16.7 / 150 (0.005) | 31 | 11 | 18.7 / 28.4 / 28.4 (0.562) |

W2 again has a meaningfully cheaper typical viewport cost, about 7.8% below
W1 on page medians, but its eleven long tasks span 61–151 ms. W1 has no
observed long task in this set; its p95 script time still exceeds 4 ms.
Presented mount p95 again exceeds 100 ms for both. All 105 confirmation
pages passed their checks with no page error. Heap and allocation were not
repeated.

### Secondary timing coverage (2026-10-25)

CPU p50 / p95 / maximum (IQR) over 15 page medians. Insert and remove
figures are per action in the declared 20-pair burst, including forced
layout from preceding changes. They are not isolated-input presentation
times. Offscreen selection alternates the last two keys; W0 retains both
rows, while they are unmounted in W1 and W2.

| Window | Items | Insert above viewport, ms | Remove above viewport, ms | Offscreen selection, µs |
| --- | --- | --- | --- | --- |
| W0 | 1,000 | 5.01 / 6.01 / 6.01 (0.330) | 6.11 / 7.35 / 7.35 (0.448) | 0.890 / 1.03 / 1.03 (0.035) |
| W0 | 10,000 | 78.1 / 80.6 / 80.6 (0.676) | 91.0 / 93.0 / 93.0 (1.38) | 0.855 / 0.920 / 0.920 (0.040) |
| W1 | 1,000 | 0.372 / 0.415 / 0.415 (0.007) | 0.422 / 0.464 / 0.464 (0.0095) | 0.295 / 0.315 / 0.315 (0.005) |
| W1 | 10,000 | 0.714 / 0.914 / 0.914 (0.0105) | 1.21 / 1.48 / 1.48 (0.0183) | 0.310 / 0.340 / 0.340 (0.015) |
| W2 | 1,000 | 0.374 / 0.391 / 0.391 (0.00975) | 0.422 / 0.442 / 0.442 (0.0115) | 0.300 / 0.320 / 0.320 (0.010) |
| W2 | 10,000 | 0.714 / 0.752 / 0.752 (0.0192) | 1.21 / 1.26 / 1.26 (0.0135) | 0.315 / 0.325 / 0.325 (0.010) |

Windowed membership changes still copy the keys and scan the aggregate, but
position only the retained window. W0 repositions every row; the next action
can force layout over that work. W1 and W2 are within variation on these
membership medians. Selecting an unmounted key updates the scalar and list
input without a row assignment. All 90 pages passed their checks with no
page error. These secondary timings change none of the threshold outcomes.

## Correctness

### Smoke (2026-10-25)

The first selection smoke completed 24 cells with two assertion failures from
the counter-mode runner defect above and no page errors. Timing cells from
that diagnostic run are not used as performance evidence. The corrected smoke
completed all 24 cells at both sizes with no failed correctness check or page
error, including counted teardown returning nodes and listeners to baseline.

The first windowing smoke completed all 30 cells with no failed correctness
check or page error, including exactly 120 marked frames per scrolling
sequence, focus inside a pinned row's input, keyboard destination mounting,
scroll anchoring and bounded row retention. Its combined sampling/tracing
timings are excluded for the instrumentation defect recorded in Method.

The repeated windowing smokes each completed 30 cells without a failed check
or page error. The final smoke includes the focused key-reuse fallback,
focused draft retention, clearing a recycled input for its next item, bulk
summary counts and all per-operation counters. The focused selection
regression covered 18 counted pages across S0, S1 and S2, with no failure.
The full Stage B set starts from this corrected version, with no further
candidate tuning planned during measurement.

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

### Stage B (2026-10-25)

All 288 pages passed their planned model checks and had no page error. Both
windows passed, at both sizes: visible order and current inputs; selected
state surviving scrolling out and back; selecting an unmounted key; no row
assignment for an unmounted label change and its updated value on entry;
retained row identity; a focused input and its draft staying mounted outside
the window; at most one extra pinned row; no focused element reused for
another item; mounting a keyboard destination before focus; deterministic
fallback for middle, last and only-item removal; focused key reuse in one
batch; clearing a draft on rebinding; and pixel scroll anchoring after
insertion and removal above the viewport. Bulk done changes derived and wrote
the summary once. Every scrolling sequence contained exactly 120 marked
frames. The six counted pages per window returned nodes, listeners and live
subscriptions to baseline after teardown.

The 105 confirmation pages passed the same model and focus checks with no
page error. Their tracing covered exactly 120 viewport-driven frames per
page. Counted teardown and allocation were not repeated in confirmation.

The secondary timing smoke passed all six cells, and all 90 measured pages
passed their post-measurement model and focus checks with no page error.

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
- Figures called CPU time are monotonic synchronous elapsed time, not thread
  CPU counters; scheduling pauses can contribute. A 15-page nearest-rank
  p95 is the maximum page value.
- CPU setup excludes deferred rendering. The input-triggered mount measures
  a separate first-use presentation boundary, including browser work.
- The common dormant item flag prevents attribution of S1's item-field
  footprint. Heap excludes native DOM bytes; profiler allocation includes
  common target preparation and varies with sampling and GC behavior.
- Windowing timing pages scroll without a focused pin; pinning and draft
  preservation are correctness checks. The timing budget with a pin is not
  established. A pinned offscreen row retains its normal binding, so the
  zero-write result applies to the unmounted offscreen workload.
- The fixture exercises successful synchronous actions. It does not establish
  asynchronous action-failure presentation or arbitrary component behavior.
- Recycling still ends and recreates item bindings and connection resources.
  The lab does not isolate each contributor to that cost or establish a
  minimum possible windowing overhead. Driven window updates also respond to
  native scroll events; unchanged-range bookkeeping is not a separate variant.
- Frame intervals are animation-frame callbacks, not presented frames.
  Long-task observer counts are primary; nested task records in traces are
  corroboration. GC figures are trace-event totals during the sequence,
  not allocator-specific attribution. Large outliers are not assigned a cause.

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

### Stage B against its thresholds (2026-10-25, first set)

| Threshold at 10,000 items | W1 | W2 |
| --- | --- | --- |
| 5. Mount CPU at most 10% of W0, presented at most 100 ms | CPU holds: 0.791%; presented fails: p95 112 ms | CPU holds: 0.778%; presented fails: p95 136 ms |
| 6. Total heap and DOM nodes at most 25% of W0 | Holds: 16.6% heap, 0.505% nodes | Holds: 16.6% heap, 0.505% nodes |
| 7. Viewport script p95 at most 4 ms, no task over 50 ms | Fails: 4.46 ms, 3 long tasks | Fails: 9.76 ms, 10 long tasks |
| 8. Unmounted label update has no DOM write; visible update within +25% of W0 | Holds for the measured workload: zero assignments; visible +1.55% | Holds for the measured workload: zero assignments; visible +0.52% |
| 9. Correctness | Holds for the listed checks | Holds for the listed checks |

The confirmation repeats the deciding timing outcomes:

| Threshold component | W1 confirmation | W2 confirmation |
| --- | --- | --- |
| 5. CPU mount ratio and presented mount | 0.796% of W0; presented p95 120 ms, fails | 0.781% of W0; presented p95 216 ms, fails |
| 7. Viewport script p95 and long tasks | 4.58 ms, zero observed long tasks; fails the 4 ms limit | 6.31 ms, 11 long tasks; fails both limits |
| 8. Visible label CPU within +25% | Holds: +0.69% | Holds: +1.38% |
| 9. Model and focus checks | Hold | Hold |

Neither window is supported against every agreed threshold in either set.
Both reduce total heap by about 83% and DOM nodes by 99.5% in this fixture,
and improve callback cadence. W2's typical viewport cost is meaningfully
lower in both sets, while its tail is worse. Heap, allocation and counted
work from the first set are not presented as repeated measurements.

The evidence favors S2 for selection CPU and footprint on the existing API,
and identifies W2 as the cheaper typical scrolling pattern in this comparison.
It does not establish the agreed all-mounted selection presentation budget,
or all windowing budgets. No threshold is moved to accept a candidate. The
owner's adoption decision is pending; no windowing helper or new public API
is authorized or implemented. Any further scope remains fixed-height rows
at the measured sizes unless the owner agrees another plan.
