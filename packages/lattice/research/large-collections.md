# Selection and windowing at 10,000 items

## Question

At 10,000 items, how should a keyed collection's single selection be held,
and does mounting only the visible rows keep mounting, memory, scrolling and
updates within budget? The answers decide whether the
[collections contract](../specs/contracts/collections.md) recommends a
selection pattern on the existing API or needs a keyed subscription API, and
whether Lattice documents a windowing pattern or proposes a windowing helper.

## Status

Planned, agreed by the owner on 2026-10-25 with the amendments below, before
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

## Conditions

Not run.

## Results

Not run.

## Correctness

Not run.

## Threats to validity

To be completed after the runs. Known in advance: fixed-height rows only;
headless frame timings; one engine.

## Decision and follow-ups

Pending measurements and the owner's decision on the reported evidence.
