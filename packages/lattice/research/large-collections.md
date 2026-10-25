# Selection and windowing at 10,000 items

## Question

At 10,000 items, how should a keyed collection's single selection be held,
and does mounting only the visible rows keep mounting, memory, scrolling and
updates within budget? The answers decide whether the
[collections contract](../specs/contracts/collections.md) recommends a
selection pattern on the existing API or needs a keyed subscription API, and
whether Lattice documents a windowing pattern or proposes a windowing helper.

## Status

Running, 2026-10-25: the owner authorizes small measurement calibration, a
shipping-API list example and comparison of the smallest mechanics proposal
it exposes. The duplicate-reconciliation guard is accepted; large-list
readiness and any new public API adoption stay open.
W1 profiling, targeted comparison
and confirmation are complete. The owner agreed the original plan
with the four requested
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
reported. The follow-up uses the plan below; no public API or helper is authorized.

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

### W1 evidence and candidate proposal (2026-10-25, before comparison)

The diagnostic smoke (one page per size) and full diagnostic set (three per
size) pass the existing model and focus checks. Every 120-step row, viewport
and end-jump sequence has 240 renders: 120 direct renders and 120 native-scroll
renders of the same range. Duplicate work is real, but entering-row lifecycle
is larger. At 10,000 items the three viewport traces attribute, per 120 steps,
127–165 ms to binding disposal, 133–190 ms to DOM removal including component
disconnection, 121–146 ms to construction plus input setters, 72–88 ms to DOM
insertion including connection, 20–26 ms to binding setup, 37–44 ms to remaining
row setup, and 41–51 ms to reconciliation plus range/position bookkeeping.
These diagnostic spans exclude nested spans from their parents. They are
instrumented attribution, not budget measurements. Trace GC totals are
51–71 ms with six major collections per viewport sequence; these overlap the
spans and are not added to them.

Sampled allocations concentrate in binding release and component disconnect
abort paths, with additional controller/composite-signal setup. Diagnostic
wrappers and the smaller sampling interval perturb allocation totals, so the
comparison will also preserve uninstrumented allocation stacks. Native DOM
allocations are outside the JavaScript heap sampler. No historical outlier
is retrospectively attributed to GC from this evidence.

The three 10,000-item diagnostic mount clicks show 2.16–2.30 ms input delay,
23.2–26.2 ms event processing and 86.3–138 ms residual to trace Event Timing's
presentation endpoint. Within that residual, main-thread style, layout,
pre-paint, paint and layer work total only 6.85–7.43 ms. Handler GC is
4.47–5.61 ms. These runs include CPU profiling and broad tracing, so their
latencies are diagnostic only. Most elapsed residual remains scheduling,
compositor/presentation or unattributed work; it is not established as slow
JavaScript or main-thread rendering. There is no evidence here to claim a
reconciliation optimization alone fixes the mount presentation budget.

Compare one combined application candidate, C1, against frozen W1:

- Skip reconciliation when start, end, focused key and membership revision
  match the completed render. Every membership callback advances the revision,
  including removal/reinsertion of an identical key. Focus changes invalidate
  the range; pinning and keyboard/fallback behavior stay intact.
- Give each row incarnation one owned binding controller. Use that same
  controller for normal disposal and setup-failure cleanup, eliminating the
  extra failure controller and composite signal.
- Supply a per-list reusable `AbortError` reason during normal binding
  disposal, avoiding a fresh default abort exception for every leaving row.
  Leave the example component's connection lifecycle unchanged.

The candidate still creates/destroys rows, retains the same native controls
and selection input, and aborts bindings before removal. It adds no public
API. All budgets, inputs, sample counts and correctness requirements above
remain fixed. First smoke the control, candidate and W0 reference cells.

### Reconciliation count amendment (2026-10-25, after comparison smoke)

The 24 comparison smoke pages pass their listed checks; no correctness or
runner failure is observed. Add precise JavaScript function-call coverage to
the separate counted pages to verify the proposed duplicate-work reduction.
Count reconciler calls over 120 viewport steps after resetting coverage.
Coverage is disabled on timing, heap, presentation and scrolling measurement
pages. Smoke these four counted cells once before the full comparison. This
adds an observed-work metric without changing any budget or candidate.

### Reconciliation counter repair (2026-10-25, counted smoke only)

The coverage smoke is a runner defect: the requested coverage counters report
stale or incomplete function-call totals inconsistent with the diagnostic
render entries. The existing correctness and teardown checks still pass.
Replace that metric with a direct counter around the imported reconciler on
counted pages only, reset before the scroll sequence and read after its final
native-scroll opportunity. Repeat only the four affected smoke cells. No
measurement-page source or candidate behavior changes.

### Allocation attribution clarification (2026-10-25, during comparison)

Uninstrumented sampled stacks can fold optimized callees into the reconciler's
frame. Report that raw bucket as reconciler/callees, not as allocations made
by the reconciliation algorithm alone. The diagnostic wrappers retain clearer
ownership boundaries but perturb stack depth and allocation. Use those passes
to identify lifecycle owners and the uninstrumented passes for total sampled
bytes. Keep unresolved attribution explicit; no timing, allocation total,
candidate or threshold is changed by this clarification.

### W1 follow-up confirmation (2026-10-25, before confirmation)

All 242 full comparison pages pass their listed correctness checks. C1 is
the sole targeted candidate. Its viewport page-median script time is 3.671 ms
against W1's 4.022 ms, a meaningful reduction, and sampled bytes fall from
228.9 to 153.8 kB per step. It still misses the 4 ms frame-script p95 budget
(4.715 ms) and the 100 ms mount presentation budget (168 ms p95). Retain those
misses and confirm unchanged W1 against unchanged C1 at 10,000 items using
the planned 15 timing, ten presentation and ten viewport-scrolling pages per
variant. No candidate, threshold or exclusion changes. Heap and allocation
remain first-set measurements.

### Bounded end-jump and native-mount investigation (2026-10-25, before runs)

The owner authorizes resolving the end-jump regression and separating mount
presentation costs, without another large run. Preserve every original sample
and trace. Analyze the original end-jump distribution by page, including the
concentration of long tasks, wall time, thread CPU and overlapping GC before
running anything new. The hypotheses are that redundant-work removal is not
itself the source of the tail, and that shared DOM/browser work explains much
of mount's residual. These remain hypotheses.

Use three unchanged 10,000-item application variants: W1, guard-only G1 and
the combined C1. G1 changes only the completed-range guard and its membership/
focus invalidation, isolating the accepted pattern from C1's controller and
disposal-reason changes. Reuse 24 px rows, the 960 px viewport, overscan and
seed. Compare 120 end jumps per fresh page, after ten warmup jumps. Take traced
and untraced passes in seeded order, with a reset/warmup before each. Report
per-page and pooled script p50/p95/max/IQR, long-task incidence, wall/thread
CPU and GC overlap. Untraced callback timers remain handler proxies.

For mount, compare unchanged W1, C1 and a minimal native-wiring control N1.
N1 constructs the same 50 initial row elements, shadow DOM, styles, input values,
attributes, positions, viewport, spacer and summary for the same 10,000-item
data. It uses native data and direct property assignments without collection/
store subscriptions. It shares the example row component and its connection
lifecycle, so this isolates state/binding/pattern cost; it does not claim to
isolate the small element helpers shared by that component. Verify equivalent
DOM shape, row inputs, geometry and summary independently after the timer.
N1 is a diagnostic control, not a candidate or a complete collection replacement.

Use real mount clicks and Event Timing for input delay, event processing and
the residual to presentation. Use a lean trace without CPU/heap sampling to
correlate handler marks, rendering, frame-task wall/thread CPU and presentation.
Keep overlapping render/raster/composite intervals separate from scheduling
wait; do not call the entire residual rendering CPU. If traces cannot identify
the off-CPU cause, use at most one sampled native-stack page per mount variant,
separate from latency measurements, or state the measurement limitation.

Smoke each of the six cells once, classify failures, then take three fresh
pages per cell: nine end-jump and nine mount pages. These small p95 values are
exploratory. Do not escalate sample counts or start a broad optimization run
without a concrete candidate or an identified measurement limitation. Keep
4 ms scrolling and 100 ms presentation targets and the existing meaningful-
difference rule. Repeatability means reporting page-level incidence, not
discarding a bad page. A broad C1 recommendation requires an explained or
bounded tail; otherwise leave it open. The guard's scoped adoption does not
depend on the combined candidate meeting every budget.

Use the existing host and local Chromium tooling, read-only shipping modules
and neutral data, with no external transmission or dependencies. All application
variants retain the existing focus, anchoring, selection, incarnation and
teardown checks. N1 receives only its explicitly scoped DOM equivalence checks.
No core redesign, windowing API, variable heights or larger item count is in scope.

### Bounded smoke repairs (2026-10-25)

The first six smoke cells have matching mount DOM snapshots and pass their
model/interaction checks, but ownership comparison uses an uncollected
navigation document and precedes automation initialization. Classify those
six teardown assertions as runner defects. Collect before the ownership
baseline and prime actionability without mounting row components. The next
six smoke cells return DOM/listener counts to that baseline.

That second smoke exposes another runner defect: delayed automation probe
click entries are mistaken for the mount click in two cells. Retain all raw
entries and correlate both API and trace Event Timing to the explicit mount
handler mark, rather than taking the first click. Only the event containing
the target handler is a mount sample. Repeat the six cells before diagnostics;
no candidate, target or correctness requirement changes.

### Bounded findings and native stacks (2026-10-25, before stack capture)

All 18 diagnostic pages pass their listed checks and all nine mount snapshots
are equivalent. At this small sample size, C1's severe 20-task end-jump page
does not recur. There is one traced C1 long task and one untraced guard-only
long task. The host is heavily loaded (one-minute load 27.79 at start on
16 logical CPUs), so isolated stalls cannot be assigned to C1 by incidence.

The identical callback timer is about twice as expensive with tracing:
W1 medians 4.470/2.215 ms and C1 4.225/1.960 ms, traced/plain. Keep these
paired callback measures distinct from the full frame-script budget; they
demonstrate observer perturbation, not that the 4 ms target has been met.
Native-wiring mount p95 is 128 ms, with about 102 ms median residual and
85 ms median off CPU inside browser frame tasks. This identifies a shared
presentation/environment limitation beyond state bindings. Use the already
planned one-second native stack capture on one fresh page per mount variant,
separate from latency evidence, to investigate the off-CPU wait. Do not start
a large timing comparison or change the 4 ms/100 ms targets.

### Calibration and runnable-example plan (2026-10-25, before calibration)

The owner assigns reusable collection presentation mechanics to the framework
where they remove repeated, error-prone work. State-model composition and
selection policy remain application conventions; keyed state and bulk commits
remain owned by `createCollection`. Elements choose markup and styling.
Reconciliation and optional windowing are candidates for callback-based
retained-item/lifetime coordination. Recycling remains experimental. This
authorizes an example and a proposal/comparison, not a new shipping API.

First calibrate unchanged W1 and C1 at 10,000 fixed-height items. Compare
untraced callbacks, a minimal timeline trace (timeline, task boundaries and
user timing), and the existing full trace. Use the same viewport/end-jump
sequences, 120 frames per pass after ten warmups, with seeded pass and page
order. Smoke one page per variant, then take three fresh pages per variant.
Keep callback timers identical across trace modes. Independently count native
scroll and ResizeObserver callbacks and correlate them with trace events;
verify 120 driver frames. Disable allocation and CPU sampling.

A trace method is eligible for further timing comparison only if its callback
perturbation is not meaningful under the existing rule (over 5% and larger
than both IQRs) and the observed callback coverage matches. If none qualifies,
record that limitation and compare mechanics with untraced synchronous
boundaries and work counts, without claiming the full frame budget is met.
No assumed scaling factor is subtracted from any result. Small-sample p95
remains exploratory; original 4.079 ms and 100 ms target misses stay recorded.

Next build a runnable fixed-height collection/list example outside the lab,
using shipping APIs and the accepted guard. Include application-owned actions
and selection, row inputs/events, explicit binding lifetimes, focus pinning,
keyboard destinations, removal fallback, anchoring and incarnation replacement.
Provide a documented local browser entry and focused behavioral coverage.
Commit this hand-written baseline independently of a framework proposal.

Record the smallest reusable surface exposed by that example before comparing
it. Keep its implementation local to the lab until the owner adopts it. Use
explicit creation, placement and release callbacks; mechanics must not choose
markup, styles, actions or selection policy. Compare identical example rows,
data and events against the hand-written baseline, including setup, teardown,
retained heap, changing ranges, same-range no-ops and key reuse. All interaction
and lifecycle checks must hold. A candidate must remove repeated ownership
work without a meaningful regression in time or retained heap. Record exact
sample counts and prototype boundaries in a dated amendment once the example
exposes them, before comparison runs.

The existing host/Chromium tooling and neutral local data are used. No external
data, installation, public exports, core redesign, variable heights or larger
item counts are authorized. Calibration and comparison are small investigations,
not a broad readiness rerun. Native presentation cost does not cancel the
additional framework handler work measured above.

### Calibration outcome and comparison boundary (2026-10-25)

The two-page smoke and six-page calibration pass callback-coverage and existing
interaction checks. Neither trace mode qualifies for subsequent timing
comparison: minimal tracing changes median callback time by 40–48%, full
tracing by 88–111%, beyond the existing meaningful-difference rule in every
cell. Use untraced synchronous setup, reconciliation and teardown boundaries
plus work counts for the mechanics comparison. These omit asynchronous
rendering and some callbacks; do not use them to claim a frame/presentation
budget pass, and do not rescale prior results.

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

### W1 diagnostic profiling (2026-10-25)

The diagnostic set used three fresh pages at each size, after one smoke page
at each size, on the same Intel Xeon W-2140B host with 16 logical CPUs and
64 GiB memory, macOS kernel 24.6.0, Node 24.12.0 and headless Chromium
156.0.8078.4. One-minute load rose from 8.57 to 11.30 during the diagnostic
set. Power state was not recorded; background workload was not controlled.
The public source revision was `da0d643f8f8f`. Shipping collection, store,
reporter, example field and reconciler hashes match Stage B.

Frozen W1 has SHA-256
`c67117497d44f00ede7c75a4b0d17096d1626e994596e31d9bb9f40eaea870f0`.
The diagnostic runner hash is
`ca9adfdcba07f18cd12cfcd30c5a5c388973c7255d65e155d2f9887e8b9c573e`;
the appended instrumentation hash is
`484d46da8469a19f056a074dfa4b766b72c22976204b4efc4ba9dbc854241e9e`.
CPU sampling used a 100 µs requested interval. Allocation sampling requested
1,024 bytes and included objects collected by minor and major GC. Diagnostic
timing, CPU sampling and allocation sampling were separate scroll passes.
Mount tracing and CPU sampling overlapped, so those click durations explain
phases rather than decide budgets. Comparison pages use the frozen source
without these diagnostic wrappers.

### W1 targeted comparison (2026-10-25)

The full comparison used the same host, runtimes, shipping source hashes and
frozen W1 as the diagnostic set. Its public source revision was
`c1aaa4f58105`. The candidate SHA-256 is
`a760d55d3955854b7b55da67b4c68c0e5e5f8b90d0cdfcfae70a9f08d214b569`;
the comparison runner hash is
`8e04d135c68bd4a70dcfb605fb1b67b504ab784447e7f942f0ef741555a55187`.
One-minute load was 11.40 at start and 9.03 at finish; five/fifteen-minute
load at finish was 12.76/15.03. Background work was uncontrolled.

There were 242 fresh pages: 96 each for W1 and C1, plus 50 W0 timing/heap
reference pages. Each size/variant used 15 timing, ten presentation, ten
scrolling, ten heap and three counted pages, excluding W0's unneeded
presentation/scroll/count cells. Timing pages kept ten rounds after five
warmups. Scroll timing used tracing without sampling; allocation was a
separate 120-step pass at a 32,768-byte requested sampling interval, including
collected objects. One browser process served fresh contexts in seeded order.

### W1 follow-up confirmation (2026-10-25)

Confirmation used 70 fresh pages at 10,000 items: 15 timing, ten presentation
and ten viewport-scroll pages for each of unchanged W1 and unchanged C1.
The host and all measured source hashes match the full comparison. The public
source revision was `a2257a5615d2`. One-minute load was 6.84 at start and
9.29 at finish. A short local trace-summary pass overlapped the opening pages;
those samples remain included. Heap and allocation sampling were not repeated.
Source hashes were checked again after confirmation and still matched.

### Bounded investigation (2026-10-25)

The bounded run used the same Intel Xeon host, 16 logical CPUs, 64 GiB,
macOS kernel 24.6.0, Node 24.12.0 and headless Chromium 156.0.8078.4.
Its public source revision was `999e76929e03`. W1, C1, shipping collection,
store, reporter, example row and reconciler hashes match the targeted
comparison. New SHA-256 values are:

| Input | SHA-256 |
| --- | --- |
| Guard-only G1 | `471ca1479aa6dee9afef01519566589a221d97c113ee729b5cb4f388fbed7c05` |
| Native wiring N1 | `6a80260ded2e7eed41ea626fe6054af872be892ed5cbadb224abefb914172923` |
| Bounded runner | `f27be8aad02e4d928e0f9995336d158a933f271715422d69a157c383042171d9` |
| Trace summarizer | `d13d510e6cfd8e7566907be143b3c3e9b689ee21746803e773fa02adb5a67b6a` |
| Native-stack runner | `e0d0efec9e8c1feff846dd8ef9487018fc0006815872e281b03f6609099cffae` |

Eighteen diagnostic pages followed the repaired six-cell smoke. End jumps
used three pages each for W1, G1 and C1, with 120 steps per traced/plain
pass after ten warmups. Mount used three pages each for W1, C1 and N1.
Automation was primed before the collected ownership baseline; no row was
constructed before the mount click. All measured mount snapshots had the
same hash, including host/shadow markup, styles, input values, positions,
geometry and summary. CPU/heap sampling was disabled during latency runs.

Host work was uncontrolled. One-minute load was 27.79 at diagnostic start
and 18.43 at finish; this is unsuitable for assigning rare scheduling tails
to a variant. Three separate one-second native-stack captures, one per mount
variant, ran at load 9.38 to 8.55. Their latency values are not deciding
samples. Native samples were aggregated over a second and lacked resolved
Chromium internal callsites and event-window stack timing. They cannot name
the blocking or descheduling cause of a particular frame. Power state was
not recorded. No broad comparison followed these diagnostics.

### Small calibration (2026-10-25)

Calibration used the same host and shipping source hashes as the bounded
investigation, Node 24.12.0 and Chromium 156.0.8078.4. One-minute load was
17.85 at start and 12.07 at finish; host work was uncontrolled. Two smoke
pages preceded six fresh measured pages, three each for W1 and C1. Each page
ran viewport and end-jump passes with no trace, minimal trace and full trace,
120 frames after ten warmups per pass. Pass/page order was seeded.

The runner SHA-256 is
`338b1f92b4c0206c598a758e01da0365eda0fd88046daaef1986e123e45db780`,
the metric implementation
`dc146dddd014096ae6cdd84a6a2ae5ee13ce4196b1f29fc0a43a94258f08139a`,
and independent callback hooks
`7963680b172173bf32852cd1972d03209e57d2ac82e4b4ccb2439ec62d173e75`.
W1/C1 and runtime modules match the earlier comparison. No CPU or allocation
sampling ran during calibration.

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

### W1 diagnostic profiling (2026-10-25)

Mount phase medians and maxima from three trace-correlated clicks per size
are below, in milliseconds. With three samples, p95 equals the maximum and
is exploratory. The presentation endpoint is Chromium Event Timing evidence,
not an animation-frame callback or a physical-display measurement.

| Items | Input delay p50 / max | Event processing p50 / max | Residual to presentation p50 / max | Total p50 / max |
| --- | ---: | ---: | ---: | ---: |
| 1,000 | 2.20 / 2.31 | 11.99 / 12.52 | 87.46 / 97.80 | 102.29 / 112.00 |
| 10,000 | 2.19 / 2.30 | 24.45 / 26.17 | 92.90 / 137.56 | 119.54 / 165.89 |

At 10,000 items, the dominant post-handler browser main-frame task spans
70.6–121.1 ms of wall time but only 8.93–15.29 ms of thread CPU time.
Its 61.7–105.8 ms difference is time off CPU inside that task; the trace
does not distinguish blocking from descheduling. Main-thread style, layout,
pre-paint, paint and layer events occupy 6.85–7.43 ms of the residual.
Across threads in that same residual interval, raster events occupy a
4.95–5.38 ms wall-time union, display draw/swap 0.87–2.25 ms, commit
0.74–0.94 ms and composite 0.60–3.10 ms. These intervals can overlap and
are not added into a critical-path total. Input queueing and the named
rendering work do not account for the whole mount miss; the large off-CPU
interval remains unresolved. No browser blocking cause is inferred.

Each row, viewport and end sequence, at both sizes and on all three pages,
observes 240 render entries for 120 steps. Exactly 120 are native-scroll
entries for the range already rendered directly. The exclusive lifecycle
breakdown and allocation evidence that motivated C1 are recorded in the
dated proposal under Method. Binding disposal and DOM removal dominate the
viewport and end-jump spans; range bookkeeping matters relatively more for
one-row steps. GC is observed separately and overlaps those spans. It is
not counted as an additional application layer.

For the 10,000-item viewport diagnostic, the median exclusive span time per
step and sampled allocation ownership are below (three pages, 120 steps per
page). Times are elapsed diagnostic spans, including any GC inside them;
allocation ownership comes from the separate sampling pass. Values are
rounded. Wrapper overhead makes these unsuitable for the 4 ms budget or
for comparing allocation totals with the uninstrumented pass.

| Diagnostic owner | Exclusive ms per step | Sampled kB per step |
| --- | ---: | ---: |
| Range and position bookkeeping | 0.200 | 13.6 |
| Reconciliation excluding timed callees | 0.157 | 22.8 |
| Binding setup and disposal | 1.25 | 124 |
| DOM construction, inputs, insertion and removal, including component lifecycle | 2.76 | 102 |
| Remaining row setup | 0.313 | 26.4 |
| Driver/other allocation | Outside these spans | 2.34 |
| GC trace time, overlapping the spans above | 0.449 | Not an application allocation owner |

The binding allocation bucket is dominated by normal release; the DOM bucket
by component disconnection. The comparison retains the latter behavior.

### W1 targeted comparison (2026-10-25, full set)

All 242 pages pass their listed correctness checks. Unless stated otherwise,
entries below are p50 / p95 / maximum (IQR), using nearest ranks. Timing
summaries use page medians; scrolling budget values pool 1,200 traced frames
from ten pages per cell. C1 is the combined application candidate described
in Method, with the shipping modules and example field unchanged.

At 10,000 items, 15 timing pages per variant:

| Variant | Synchronous mount, ms | Teardown, ms | Visible label update, µs |
| --- | ---: | ---: | ---: |
| W0 reference | 922 / 1,026 / 1,026 (56.4) | 173 / 201 / 201 (15.1) | 2.91 / 3.17 / 3.17 (0.055) |
| Unchanged W1 | 7.365 / 9.870 / 9.870 (1.240) | 0.870 / 1.190 / 1.190 (0.105) | 2.92 / 3.94 / 3.94 (0.125) |
| C1 | 7.255 / 8.180 / 8.180 (0.635) | 0.645 / 0.710 / 0.710 (0.050) | 2.96 / 3.13 / 3.13 (0.120) |

Mount's median difference is not meaningful. Teardown is 25.9% cheaper,
clearing both IQRs. C1's mount is 0.787% of W0 and its visible update is
1.72% above W0, within their relative limits. Unmounted label updates assign
no row input. Their CPU medians are 0.325 µs for both windows, versus
2.90 µs for W0's mounted row. Bulk update medians are 3.465 ms for W1,
3.480 ms for C1 and 41.695 ms for W0; both windows still derive/write the
summary once per commit.

Heap at 10,000 items, ten pages per variant, in decimal MB:

| Variant | Total heap | Presentation-only heap | DOM nodes, constant |
| --- | ---: | ---: | ---: |
| W0 reference | 9.1133 / 9.1157 / 9.1157 (0.0023) | 7.6460 / 7.6484 / 7.6484 (0.0023) | 100,005 |
| Unchanged W1 | 1.5105 / 1.5168 / 1.5168 (0.0061) | 0.04390 / 0.04999 / 0.04999 (0.00609) | 505 |
| C1 | 1.5102 / 1.5163 / 1.5163 (0.0002) | 0.04362 / 0.04971 / 0.04971 (0) | 505 |

C1 retains 16.6% of W0's total heap and 0.505% of its DOM nodes. Its heap
difference from W1 is not meaningful. Total heap, rather than just the much
smaller presentation increment, decides the heap threshold.

Mount click Event Timing at 10,000 items, ten events per variant, ms:

| Variant | Input delay | Event processing | Residual to presentation | Total |
| --- | ---: | ---: | ---: | ---: |
| Unchanged W1 | 1.58 / 1.88 / 1.88 (0.18) | 19.07 / 22.51 / 22.51 (1.36) | 83.87 / 103.60 / 103.60 (15.38) | 104 / 128 / 128 (16) |
| C1 | 1.59 / 1.78 / 1.78 (0.12) | 19.17 / 20.29 / 20.29 (1.42) | 91.86 / 145.93 / 145.93 (38.93) | 112 / 168 / 168 (40) |

The mount presentation budget fails again. The median difference is smaller
than both IQRs; there is no supported mount latency improvement. Presented
selection p95 is 16 ms for both windows, with observed maxima 24 ms. Event
Timing emitted 199 of 200 W1 selection clicks and 197 of 200 C1 clicks;
all clicked selections were checked, and missing entries are not imputed.

Viewport scrolling at 10,000 items:

| Variant | Frame script, ms | Page-median script, ms | Sampled kB/step, ten pages | GC trace ms/sequence, ten pages | Long tasks |
| --- | ---: | ---: | ---: | ---: | ---: |
| Unchanged W1 | 4.061 / 5.545 / 139.289 (0.265) | 4.022 / 5.545 / 5.545 (0.187) | 228.9 / 242.5 / 242.5 (11.6) | 23.58 / 53.68 / 53.68 (2.26) | 7, 57–149 ms |
| C1 | 3.700 / 4.715 / 26.422 (0.295) | 3.671 / 4.155 / 4.155 (0.122) | 153.8 / 159.8 / 159.8 (7.1) | 16.43 / 46.11 / 46.11 (5.88) | 0 |

C1's typical viewport script reduction is 8.73%, exceeding 5% of control
and both page-median IQRs. Allocations fall 32.8%, also clearing that rule.
The 4 ms p95 limit still fails. The observed GC time reduction is 30.3%;
GC intervals overlap application work and are not added to it. Median major
collection counts are six for W1 and five for C1 per 120-step trace.
All three counted pages at each size observe 240 reconciler calls for W1
and 120 for C1. The counted middle-viewport step still creates/removes 40
rows for both variants; the candidate skips duplicate reconciliation, not
necessary row lifetimes.

Uninstrumented viewport allocation stacks, median kB per step with IQR:

| Stack owner | Unchanged W1 | C1 |
| --- | ---: | ---: |
| Range/position | 15.1 (2.18) | 6.57 (2.74) |
| Reconciler/callees, including optimized lifecycle callees | 77.6 (8.28) | 12.1 (2.23) |
| Visible binding lifecycle frames | 24.3 (4.10) | 25.1 (3.28) |
| DOM/component | 82.6 (6.91) | 80.1 (5.57) |
| Remaining row setup | 28.2 (2.20) | 23.3 (3.81) |
| Driver/other | 2.46 (1.36) | 2.19 (0.84) |

Bucket medians need not sum to the median total. The large ancestor-frame
reduction is consistent with the lifecycle changes identified by diagnostic
wrappers; it is not evidence that a reconciler's set construction alone
allocated 77.6 kB. Component disconnection remains a large allocation site.

Other scrolling inputs at 10,000 items, ten pages and 1,200 frames per cell:

| Input / variant | Frame script, ms | Page-median script, ms | Sampled kB/step | Long tasks |
| --- | ---: | ---: | ---: | ---: |
| Row / W1 | 0.739 / 0.827 / 1.019 (0.067) | 0.726 / 0.784 / 0.784 (0.033) | 36.4 / 40.3 / 40.3 (3.86) | 0 |
| Row / C1 | 0.640 / 0.715 / 0.851 (0.058) | 0.637 / 0.673 / 0.673 (0.039) | 17.8 / 23.5 / 23.5 (2.45) | 0 |
| End / W1 | 4.576 / 5.021 / 7.355 (0.211) | 4.557 / 4.743 / 4.743 (0.215) | 259.8 / 271.1 / 271.1 (11.7) | 0 |
| End / C1 | 4.217 / 11.849 / 177.466 (0.270) | 4.189 / 11.660 / 11.660 (0.127) | 173.0 / 182.2 / 182.2 (12.6) | 21, 54–288 ms |

The row and end typical reductions clear the meaningful-difference rule,
but C1 has a worse end-jump tail. Those samples are retained. Trace correlation
finds W1's seven viewport long tasks at 57.8–149.2 ms wall time with only
11.4–33.2 ms thread CPU and no overlapping main-thread MinorGC/MajorGC.
C1's 21 end-jump long tasks span 54.3–288.9 ms wall and 7.38–46.43 ms
thread CPU; one overlaps 8.60 ms of main-thread GC, the others none. Much
of these stalls is off CPU, including intervals inside scripting, paint or
layout spans. The trace does not identify the blocking/descheduling cause.
They cannot all be attributed to allocation or GC. End-jump tails are not
repeated by the planned viewport-only confirmation.

At 1,000 items, the same sample counts give W1/C1 mount medians of
2.530/2.430 ms, teardown 0.870/0.610 ms, total heap 0.1808/0.1805 MB and
presentation heap 0.04378/0.04351 MB, with 505 nodes each. Mount presentation
is 96/144/144 (24) ms for W1 and 96/160/160 (24) ms for C1. Viewport
script is 4.122/9.112/138.188 (0.571) ms for W1 and
3.593/4.710/36.454 (0.280) ms for C1. Page-median script is
4.047/8.505/8.505 (0.304) versus 3.550/4.411/4.411 (0.129) ms, a
meaningful 12.3% typical reduction. Sampled viewport bytes fall from
224.8 to 155.4 kB/step; viewport long tasks fall from 16 to zero. Row
script p95 is 1.445/0.761 ms and end-jump p95 5.297/4.505 ms, W1/C1,
with no row or end-jump long tasks at this size. The agreed budgets decide
at 10,000 items; the smaller fixture does not rescue a missed budget.

### W1 follow-up confirmation (2026-10-25)

All 70 pages pass their listed correctness checks. Entries are p50 / p95 /
maximum (IQR), with the same timing boundaries and nearest-rank summaries as
the full set. Fifteen timing pages per variant:

| Variant | Synchronous mount, ms | Teardown, ms | Visible label update, µs |
| --- | ---: | ---: | ---: |
| Unchanged W1 | 7.190 / 7.405 / 7.405 (0.120) | 0.865 / 0.885 / 0.885 (0.020) | 2.895 / 2.975 / 2.975 (0.075) |
| C1 | 7.030 / 7.835 / 7.835 (0.275) | 0.620 / 0.670 / 0.670 (0.040) | 2.885 / 3.075 / 3.075 (0.105) |

Mount's median difference remains below 5%. Teardown is meaningfully lower,
by 28.3%. Visible update medians are effectively unchanged.

Mount Event Timing, ten events per variant, in milliseconds:

| Variant | Input delay | Event processing | Residual to presentation | Total |
| --- | ---: | ---: | ---: | ---: |
| Unchanged W1 | 1.54 / 1.68 / 1.68 (0.075) | 18.585 / 20.100 / 20.100 (0.290) | 83.16 / 90.25 / 90.25 (7.98) | 104 / 112 / 112 (8) |
| C1 | 1.51 / 1.60 / 1.60 (0.085) | 18.585 / 19.155 / 19.155 (0.370) | 83.41 / 91.32 / 91.32 (7.62) | 104 / 112 / 112 (8) |

Both mount p95 values still exceed 100 ms. Selection clicks have complete
coverage of 200 events per variant, all reported at 16 ms. The candidate
does not reduce the dominant mount presentation residual.

Viewport scrolling, 1,200 frames from ten pages per variant:

| Variant | Frame script, ms | Page-median script, ms | GC trace ms/sequence | Long tasks |
| --- | ---: | ---: | ---: | ---: |
| Unchanged W1 | 4.100 / 4.561 / 5.818 (0.381) | 4.072 / 4.220 / 4.220 (0.083) | 20.11 / 23.81 / 23.81 (1.05) | 0 |
| C1 | 3.701 / 4.079 / 4.437 (0.323) | 3.700 / 3.741 / 3.741 (0.033) | 17.29 / 21.05 / 21.05 (0.92) | 0 |

The typical reduction is 9.14%, clearing 5% and both IQRs again. The p95
limit still fails at 4.079 ms; it is not rounded down to accept the candidate.
Neither variant has a long task in this confirmation. Callback cadence is
16.665 ms median and 16.670 ms p95/max for both; it remains a callback proxy,
not a presented-frame metric. Median major collections are four for W1 and
three for C1. The GC-time direction repeats, with a smaller 14.1% reduction.
Heap, allocation and the full set's end-jump regression are not reconfirmed.

### Bounded end-jump investigation (2026-10-25)

The original 1,200-frame-per-variant comparison remains intact. C1's pooled
end-jump p95 was 11.849 ms against W1's 5.021 ms, 2.36 times as large;
maximum script time was 177.466 versus 7.355 ms. C1 had long tasks on two
of ten pages, W1 on none. Twenty of C1's 21 long tasks occurred on one page:
that page's script median/p95/max was 11.660/103.987/177.466 ms. The other
affected page had one 54 ms long task and a script median of 4.421 ms.
The other nine C1 page medians ranged from 4.096 to 4.421 ms. This is a large,
concentrated tail regression, not a uniform end-jump slowdown.

The original trace correlation identifies an off-CPU stall class: 54.3–288.9 ms
wall time versus 7.38–46.43 ms thread CPU in those long tasks, with only one
overlapping 8.60 ms of main-thread GC. That does not establish C1 as their
cause. A slow operation's elapsed trace span includes time its thread did
not run; a long paint or script span is not all paint or script CPU work.

Bounded repeat, three pages and 360 frames per traced cell, p50 / p95 /
maximum (IQR), in milliseconds:

| Variant | Traced frame script | Traced page-median script | Long tasks, traced / plain |
| --- | ---: | ---: | ---: |
| W1 | 4.810 / 5.446 / 6.326 (0.310) | 4.830 / 4.832 / 4.832 (0.046) | 0 / 0 |
| Guard-only G1 | 4.777 / 5.538 / 5.964 (0.385) | 4.915 / 4.968 / 4.968 (0.381) | 0 / 1 |
| Combined C1 | 4.482 / 5.345 / 9.666 (0.484) | 4.477 / 4.842 / 4.842 (0.518) | 1 / 0 |

The severe C1 page does not recur in this small repeat. Its one traced long
task lasts 101.816 ms, uses 8.765 ms thread CPU, and overlaps no main-thread
MinorGC/MajorGC. It is mostly off CPU outside the measured script spans.
G1's untraced pass has one 83 ms long task and a 74.630 ms callback maximum;
that pass has no trace to attribute its CPU/GC. Neither is discarded.
The three-page result does not bound the probability of rare stalls or prove
them absent. G1 does not reproduce the original concentrated regression,
and the data do not establish a guard-specific or C1-specific cause.

The paired callback timer exposes substantial tracing perturbation:

| Variant | Traced callback, ms | Plain callback, ms |
| --- | ---: | ---: |
| W1 | 4.470 / 5.055 / 5.930 (0.290) | 2.215 / 2.525 / 3.010 (0.125) |
| G1 | 4.525 / 5.255 / 5.640 (0.370) | 2.300 / 3.175 / 74.630 (0.250) |
| C1 | 4.225 / 5.060 / 9.360 (0.480) | 1.960 / 2.260 / 2.980 (0.090) |

These use the same callback boundary within a page; tracing approximately
doubles its median. The plain timer omits native-scroll and other callbacks,
so it is not a substitute for the full frame-script metric and does not
establish a 4 ms pass. Pass ordering was seeded and warmups preceded both.
An accurate low-overhead frame-work method needs validation before another
large readiness run. The recorded 4.079 ms remains a miss.

The observed cause class is off-CPU delay plus material measurement overhead;
the exact blocking/descheduling cause remains unidentified. The loaded shared
host, limited native symbols and aggregate stack windows prevent a causal
assignment to the candidate. General C1 recommendation therefore remains open.

### Bounded native-mount comparison (2026-10-25)

Three mount events per variant, p50 / p95 / maximum (IQR), in milliseconds.
Small-sample p95 equals the maximum and is exploratory. These are API Event
Timing phases correlated to the actual mount handler, not the automation
probe or an animation-frame callback:

| Variant | Input delay | Event processing | Residual to presentation | Total |
| --- | ---: | ---: | ---: | ---: |
| W1 | 1.31 / 1.40 / 1.40 (0.22) | 21.695 / 21.925 / 21.925 (0.460) | 97.36 / 128.90 / 128.90 (32.14) | 120 / 152 / 152 (32) |
| C1 | 1.26 / 1.27 / 1.27 (0.25) | 20.800 / 21.965 / 21.965 (1.680) | 98.46 / 104.76 / 104.76 (14.59) | 120 / 128 / 128 (16) |
| Native wiring N1 | 1.26 / 2.31 / 2.31 (1.06) | 8.765 / 8.850 / 8.850 (0.495) | 102.39 / 117.89 / 117.89 (24.97) | 112 / 128 / 128 (24) |

State construction, subscription setup and pattern work add roughly 12–13 ms
of median handler time over native wiring in this fixture. That difference
clears both IQRs and 5%; it is not an allocation to an individual framework
function. N1 shares the row component and its small element helpers, so
their cost is not subtracted. Total presentation differences do not clear
both IQRs. Native wiring itself exceeds the unchanged 100 ms target.

Trace medians, with full per-page ranges in parentheses, in milliseconds:

| Variant | Main-thread rendering CPU | Raster wall-time union | Frame-task wall / CPU | Off CPU inside frame tasks |
| --- | ---: | ---: | ---: | ---: |
| W1 | 6.56 (6.45–6.65) | 5.26 (4.99–5.70) | 81.84 / 4.72 | 77.33 (74.42–101.59) |
| C1 | 6.42 (6.28–6.44) | 5.33 (5.25–5.37) | 84.52 / 4.33 | 80.34 (75.78–83.37) |
| N1 | 6.67 (6.42–6.71) | 5.34 (5.27–5.44) | 89.99 / 4.72 | 85.34 (71.43–101.47) |

Rendering here unions nested style/layout/pre-paint/paint/layer spans on the
renderer main thread; raster is on other threads and may overlap. Main frame
tasks do not contain every rendering event, so their CPU column is not the
same scope as the rendering column. The handler-to-first-frame interval is
about 5.0–5.3 ms across these pages and includes rendering work before that
frame task; it is not pure queueing delay. After the last frame task,
presentation takes another 1.69–14.61 ms. Trace presentation endpoints are
unquantized; API durations above are quantized. These intervals are not
summed into a fabricated rendering/scheduling split.

The dominant residual persists with native wiring and equivalent DOM, while
named rendering CPU and raster work stay similar. It belongs to shared
DOM/component/browser/environment behavior rather than collection subscriptions
alone. Time off CPU separates running work from non-running time but does not
distinguish a browser wait from OS descheduling. Native sampling completes
for all three variants but gives largely unresolved Chromium frames and
aggregate message-loop waits across idle and active time. It cannot identify
the critical frame's wait or justify a font, compositor or scheduler diagnosis.

This establishes a concrete measurement limitation, not general list readiness.
No core redesign or broad candidate recommendation follows. The next diagnostic
needs low-overhead, coverage-equivalent script accounting and a time-correlated,
symbolized scheduling capture on a controlled host. No large run was started.

### Small calibration (2026-10-25)

Page-median callback time, three pages per cell, p50 / p95 / maximum (IQR),
in milliseconds. This is the same synchronous callback boundary in all modes:

| Variant / input | No trace | Minimal trace | Full trace |
| --- | ---: | ---: | ---: |
| W1 / viewport | 1.970 / 2.040 / 2.040 (0.150) | 2.760 / 2.795 / 2.795 (0.145) | 3.705 / 3.715 / 3.715 (0.030) |
| W1 / end jump | 2.240 / 2.240 / 2.240 (0.065) | 3.215 / 3.315 / 3.315 (0.190) | 4.440 / 4.605 / 4.605 (0.230) |
| C1 / viewport | 1.700 / 1.810 / 1.810 (0.125) | 2.520 / 2.585 / 2.585 (0.085) | 3.445 / 3.570 / 3.570 (0.155) |
| C1 / end jump | 1.950 / 1.985 / 1.985 (0.045) | 2.875 / 2.895 / 2.895 (0.040) | 4.105 / 4.185 / 4.185 (0.155) |

Minimal tracing adds 40.1%, 43.5%, 48.2% and 47.4%, respectively; full
tracing adds 88.1%, 98.2%, 102.6% and 110.5%. Each exceeds 5% and both
IQRs. Both traces cover all independently counted callbacks: 120 driver
frames, 119 native-scroll callbacks and 4,760 ResizeObserver callbacks per
viewport pass, or 5,950 ResizeObserver callbacks per end-jump pass. The last
native scroll/observer delivery is outside the original stop mark, and is
excluded by both the trace and independent counters. No coverage mismatch
or smoke failure occurs.

The minimal trace improves perturbation but does not eliminate it. Neither
method is eligible under the predeclared rule. No factor is subtracted from
an existing result, and plain callback time is not full frame-script time.
Keep the 4.079 ms miss and 4 ms/100 ms targets unchanged. Subsequent mechanics
timing compares matching untraced synchronous calls and reports that scope.

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

### W1 follow-up (2026-10-25)

The two diagnostic smoke pages and six diagnostic pages pass the existing
model and interaction checks. The 24 comparison smoke pages also pass. All
four coverage-counter smoke pages fail only the added counter assertion:
they report nine calls for W1 and five for C1, stale/incomplete counts.
These are runner defects, not candidate findings. The four repaired smoke
pages pass with direct counts of 240 and 120 respectively.

All 242 comparison and 70 confirmation pages pass the listed checks without
a page error. These include same-selection no-ops, offscreen selection,
retained row/control identity and draft input, one additional pinned focused
row, mounting a keyboard destination before focus, deterministic middle/end/
empty removal fallback, insertion/removal scroll anchoring, independent and
batched key reuse with a new incarnation, focused key-reuse fallback, model
output and summary updates. Counted teardown pages return DOM nodes and
listener counts to baseline and leave zero live subscriptions. No runtime
source or public API was changed. No lint, typecheck, formatting check or
package test suite was run; verification was the authorized lab coverage.

### Bounded investigation (2026-10-25)

The first six smoke cells fail only the invalid ownership-baseline comparison;
the next six fix ownership but two fail stale mount-event matching. Both are
runner defects, with raw records retained. The repaired six-cell smoke passes.
All 18 diagnostic and three native-stack pages pass their scoped checks.
All nine diagnostic mount snapshots match exactly across W1, C1 and N1.

W1, G1 and C1 retain the existing model, no-op, offscreen selection, focused
row pinning, keyboard destination, deterministic focus fallback, insertion/
removal anchoring and key-incarnation checks. Teardown returns nodes and
listeners to each page's collected automation-primed baseline. N1 is checked
only for its declared initial DOM, inputs, geometry, summary and teardown;
it is not a complete list candidate. Original samples and traces are unchanged.
No core source, public API or windowing helper changed. Lint, typecheck,
formatting and package tests were not run.

### Calibration and example verification (2026-10-25)

All eight calibration pages pass callback coverage and the inherited focus,
anchoring, incarnation, selection and teardown checks. The runnable example's
focused capability and Chromium tests exercise its application actions and
interaction ownership. Its actual HTTP entry is exercised with 10,000 items,
bounded initial mounting, a native edit, selected-key replacement and teardown.
These checks establish the example's listed behavior, not large-list budgets.
No lint, typecheck, formatting or full test suite ran.

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

### W1 follow-up limits (2026-10-25)

Diagnostic hooks add calls and stack depth, including to default abort error
stacks. Their span and allocation values explain ownership but do not decide
budgets. Uninstrumented V8 allocation stacks can omit optimized callees;
reconciler/callee bytes are not all reconciliation bookkeeping. Native DOM
allocation is not measured by the JavaScript sampler.

The candidate combines a range guard, simpler binding ownership and a reused
disposal reason. Its overall comparison does not establish the independent
benefit of each change. The example component's connection/disconnection
behavior remains a measured cost. Browser thread-time evidence identifies
large off-CPU mount intervals but does not identify their blocking or
scheduling cause. Headless Event Timing is presentation evidence for this
browser surface, not a physical-display measurement.

The fixture remains fixed-height at 1,000 and 10,000 items. Its interaction
checks do not establish every page layout, asynchronous failure workflow or
accessibility behavior. Background host work and power state are uncontrolled.

### Bounded investigation limits (2026-10-25)

Three pages per cell cannot establish rare-stall probability. The repeat was
on a heavily loaded shared host and did not replay the original machine
scheduler state. The traced/plain comparison demonstrates instrumentation
overhead within identical callback boundaries; the plain callback omits some
frame scripting and cannot establish readiness against the 4 ms metric.
Native wiring shares the real row component, including lifecycle and element
helpers. It isolates state/binding work without isolating every Lattice module
or every browser/OS contributor. One-second native stack aggregates lack the
symbol resolution and event-window timing needed to name a frame's wait.
These limitations apply to interpretation, not to the unchanged targets or
the requirement to preserve adverse samples.

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

### W1 targeted follow-up against its budgets (2026-10-25)

| Existing threshold at 10,000 items | C1 full comparison | C1 confirmation |
| --- | --- | --- |
| 5. Mount CPU at most 10% of W0, presented at most 100 ms | CPU holds at 0.787%; presented fails at 168 ms p95 | Presented fails at 112 ms p95; no new W0 reference |
| 6. Total heap and nodes at most 25% of W0 | Holds: 16.6% total heap, 0.505% nodes | Not repeated |
| 7. Viewport script p95 at most 4 ms, no task over 50 ms | Fails at 4.715 ms; zero long tasks | Fails at 4.079 ms; zero long tasks |
| 8. Unmounted update has no DOM write; visible CPU within +25% of W0 | Holds: zero row assignments, visible +1.72% | Visible cost unchanged against W1; no new W0 reference |
| 9. Correctness | Holds for the listed checks | Holds for the listed checks |

The evidence supports the targeted application's lower work: reconciling a
completed range is unnecessary, one row controller owns normal and failure
cleanup, and a reusable normal-disposal reason reduces transient allocation.
The combined candidate reduces typical viewport script time by 8.73% in the
full set and 9.14% in confirmation, with both differences clearing the agreed
rule. Sampled allocations fall 32.8% in the full set, and teardown is cheaper
in both sets. Heap differs negligibly from W1.

This does not support C1 as a pattern meeting all agreed budgets. Mount
presentation remains over 100 ms and viewport script p95 remains over 4 ms.
The full set's end-jump tail regresses and is not reconfirmed. The largest
remaining measured application cost is the native row/component lifecycle;
the large off-CPU browser intervals need further attribution before assigning
a cause or proposing a specific fix for them. Neither API changes nor a
windowing helper follow from this application-pattern comparison. The owner
decides adoption; no product implementation or scope expansion is authorized
by these results.

### Scoped owner adoption (2026-10-25)

The owner adopts only C1's duplicate-reconciliation guard as a documented
[application pattern](../specs/framework/collections.md). It removes demonstrated
redundant work. This does not adopt the combined candidate generally, its
controller/disposal changes, or large-list readiness. The 4.079 ms result
remains a miss against 4 ms; the 100 ms mount presentation target is unchanged.
End-jump repeatability/cause and native mount comparison remain open. Delivery
and navigation work are independent of these investigations; a large-list
migration remains gated on its own readiness evidence.

### Bounded investigation outcome (2026-10-25)

The completed-reconciliation guard remains the owner's accepted application
pattern. The combined C1 candidate is not generally recommended: the original
large end-jump tail is concentrated, is not reproduced at that magnitude in
the bounded repeat, and lacks a candidate-specific causal explanation. The
observed stalls are mostly off CPU; GC does not explain most of them.

The native comparison establishes that collection/store binding overhead is
not the dominant mount presentation residual for this DOM. It adds measurable
handler work, while a comparable residual and target miss remain in the native
control. Tracing materially perturbs scrolling work, and the shared host does
not permit reliable attribution of rare scheduling stalls. Stop at these
measurement limitations rather than launch another large comparison or infer
a core fix. Validate low-overhead measurement and obtain event-correlated
scheduling evidence before general C1 adoption or a readiness claim.

The 4 ms and 100 ms targets remain unchanged; 4.079 ms remains a miss. Large-list
readiness and migration stay gated. Delivery and navigation remain independent
and unblocked. No collection-core redesign or windowing helper is adopted.
