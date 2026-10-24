# Full flow against hand-written wiring

## Question

What does Lattice's state and binding layer cost over equivalent hand-written
wiring, for the same elements and the same visible behavior? The flow measured
is store → capability → binding → element, including setup and teardown.

The answer informs where Lattice needs work before more is built on it: a
binding helper, a cheaper setup, a field index, or none. It does not choose
between Lattice and hand-written code; it locates and sizes Lattice's overhead.
Helpers are not proposed until this result and the example's repetition say
where they would help.

## Status

Decided, 2026-10-24. The owner agreed to the plan on 2026-10-24 and asked for
it to run at once rather than wait for a quieter machine. Decision: keep the
current design. The lab supports negligible visible interaction overhead for
these workloads, with measurable setup, teardown and memory costs; it does not
show that Lattice is free or faster than hand-written wiring.

## Hypothesis

- **Interaction cost.** An interaction is dominated by the browser's input,
  style, layout and paint work, as in the
  [component authoring](component-authoring.md) lab, where script was about
  0.1 ms of 2.3 ms per interaction at 10 elements. Lattice adds a field check
  per subscription, a `read` and a comparison per considered subscription, and
  a function call per assignment. Interaction cost is expected to be within 5%
  of hand-written wiring at 10 fields, with script duration measurably higher.
- **Setup and teardown.** Each field binding allocates an internal controller,
  a combined signal, a `present` function, two subscription records with their
  closures and abort listeners, and an event listener. Hand-written wiring
  allocates one listener per field. Setup time and memory per mounted field are
  expected to be clearly higher with Lattice: a multiple of the wiring's own
  cost, though small next to the element's.
- **Unchanged and unrelated updates.** Both variants skip them: hand-written
  wiring by its own comparisons, Lattice by `Object.is` in `set` and by field
  checks. Both are expected to make no DOM writes; Lattice is expected to cost
  more per action in script and allocations, from the function patch and the
  commit walk.

This is reasoning, not evidence.

## Method

### Variants

Both variants mount the same elements, unchanged, from the
[filters example](../examples/filters/): `<acme-filter-field>` and
`<acme-filter-summary>`.

| Variant | Role | State and wiring |
| --- | --- | --- |
| 0. Hand-written wiring | Control | The query, lock and revision in local variables. Actions compare with the current value, update the variable and assign the affected element inputs directly, deriving the summary text and assigning it only when it changed. One listener per field calls the query action, with the same failure presentation as the binding. Teardown aborts one controller, which removes the listeners, and removes the elements |
| 1. Lattice | Candidate | The filters example unchanged: `createFilters` on `createStore`, `bindFilterField` and `bindFilterSummary`, `mountFilters` |

Both have the same visible behavior and the same guarantees the example's
browser tests check: initial values, updates to every field and the summary,
no assignment for unchanged and unrelated updates, presentation of a failed
save, and complete teardown. The control is checked against those same tests
before any run. `save` is a no-op in both during measurement.

### Workloads and sizes

| Workload | Sequence |
| --- | --- |
| Setup | Create the capability or the variables, the elements and the wiring, and connect them |
| Teardown | Abort the lifetime: release the wiring and remove the elements |
| Typing | A real key press through Playwright into the first field: native `input`, intent event, query action, every field's `value` and the summary's `text` assigned |
| Lock | A real click on a button that calls the lock action: every field's `disabled` assigned |
| Unchanged | The query action with the current query, called from the page |
| Unrelated | The revision action, which no element shows, called from the page |
| Churn | 20 cycles of mount, one typing interaction, teardown |

Sizes: 1, 10 and 100 fields, plus the summary. 10 is the primary size.

### Measures

| Measure | Evidence |
| --- | --- |
| Interaction cost | Profiler: `Performance.getMetrics` deltas of `TaskDuration` and `ScriptDuration` per typing and lock interaction, in blocks of 21 interactions with a frame after each, as in the component authoring lab |
| Setup and teardown time | Observed: `performance.now()` around each, in cross-origin isolated pages, in batches reaching at least 1,000 fields per repetition, per field |
| Unchanged and unrelated cost | Observed: `performance.now()` around 1,000 calls of the action, per call |
| Allocations | Profiler: bytes allocated during 1,000 unchanged actions, 1,000 unrelated actions and 100 typing interactions, from Chrome's sampling heap profiler at a 64-byte interval, collected objects included |
| Memory per mounted field | Profiler: JavaScript heap after garbage collection, mounted minus empty, at 100 fields, divided by 100 |
| Retained after churn | Profiler: JavaScript heap and `Memory.getDOMCounters` listener and node counts after garbage collection, after the churn workload, against before it |
| Selection and derivation calls | Observed: calls to each selection's `read` in Lattice and to the summary derivation in the control, per action |
| DOM writes | Observed: spies on the elements' input setters and on the control's `value` and `disabled` setters, `Text.data` and attribute methods, per action |
| Cleanup | Observed: after teardown, observers disconnected, no input assigned and no action called for further actions and events, and listener counts back to their baseline |

Counted measures run in their own pages, apart from clean timing pages, and
allocation sampling runs in its own pages too.

### Runs

Each sample is a fresh browser context loading one variant, in an order
shuffled with seed 1. Clean timing: 20 pages per variant at 10 fields and 10 at
the other sizes. Counted: 3 pages per variant and size. Allocation sampling and
memory: 10 pages per variant and size. Warmups: one block or 5 repetitions per
workload before the measured ones, as in the component authoring lab.

### Correctness checks

Every run checks, in both variants: initial values; typing updates every field
and the summary; lock updates every field; unchanged and unrelated actions make
no DOM write; after teardown, actions assign nothing, events call nothing,
observers are disconnected and listener counts return to the baseline. A
variant that fails a check in any page is not reported as comparable.

### Metrics and how results are read

There is no adoption decision. Results are reported as Lattice's overhead over
the control, absolute and relative, with sample counts and spread:

- **Material overhead** is a difference larger than 5% of the control and than
  both variants' interquartile ranges. Below that it is reported as no
  measurable overhead.
- Interaction cost at 10 fields is the headline figure. Setup, teardown,
  unchanged, unrelated, allocations and memory are each reported with their
  absolute cost per field or per action, so that a large relative overhead on a
  tiny absolute cost reads as such.
- A follow-up is proposed only for a material overhead: for example a binding
  helper or a cheaper setup if setup and memory dominate, or a field index if
  the commit walk does. Each proposal needs its own comparison.

### Amendment before the first run (2026-10-24)

- **Control correctness.** The control lives in the lab, so the example's own
  browser test file cannot run against it. Instead every counted page runs the
  same cases on both variants: initial values, typing, locking, no writes for
  unchanged and unrelated actions, and complete teardown.
- **Counting selection calls.** `mountFilters` creates its capability and binds
  it in one call, so its selections cannot be wrapped from outside. Counted
  pages for the Lattice variant mount through a copy of `mountFilters` that
  wraps each selection's `read` before binding, using the example's own
  `createFilters`, bindings and elements. Timing, allocation and memory pages
  use `mountFilters` unchanged.
- **Derivation count in the control.** The control counts its summary
  derivations with one increment in every page, including timing pages; that
  increment is a small cost the Lattice variant does not pay.
- **Allocation sampling.** Chrome's sampling heap profiler reports estimated
  bytes allocated. The sum of every sampled node's self size is reported, with
  objects collected by minor and major collections included.

### Stage 2: allocations of writes (2026-10-24, before any stage 2 run)

**Attribution, done first as the owner asked.** Chrome's sampling heap
profiler at a 32-byte interval attributed allocations of the Lattice variant
after 20,000 warmup calls, 20,000 sampled calls, 5 pages each at 10 and 100
fields. A diagnostic run with inlining disabled split the store's functions
apart:

| Write | Site | Bytes per call | What it is |
| --- | --- | --- | --- |
| Unrelated | `set` | 210 | The new snapshot, and two growable arrays of changed keys and values |
| Unrelated | `commit` | 97 | A commit record object, and the queue's backing store, regrown after `queue.length = 0` trims it |
| Unrelated | `touch` and its patch arrow | 45 | The application's patch function and patch object |
| Unchanged | `setQuery` | 16 | The application's patch object; `set` allocates nothing |

The delivery walk allocates nothing in optimized code, at either size. The
stage 1 allocation figures that grew with subscriptions were sampled after 300
warmup calls, before optimization; they describe early, unoptimized execution,
not steady state. The walk's time cost, which stage 1 measured after 5,000
calls, still grows with subscriptions.

**Question.** Does a targeted change to `set` and the commit queue reduce the
steady-state allocations of a changing write without costing time?

**Variants.** Variant 1 is the current store, unchanged. Variant 2 is a lab
copy of the store with two changes, served in place of `store.js`:

- `set` builds the next snapshot when it finds the first changed key and
  writes changed values into it during the same walk, keeping only an
  exact-size array of changed keys instead of two growable arrays. Nothing is
  published before the walk ends, so a rejected write still changes nothing,
  and an unchanged write still allocates nothing.
- The queue keeps its capacity: commits are held in two parallel arrays,
  versions and changed keys, with a count, and no commit object. After
  delivery the count resets and the slots are cleared so no keys stay
  referenced.

Both variants run the filters example unchanged.

**Measures.** Bytes per unrelated, unchanged and typing write after 20,000
warmup calls, sampled at 32 bytes, 10 pages per variant at 10 and 100 fields.
Time per unrelated and unchanged action, interaction task and script time,
setup and teardown, with the stage 1 timing workloads, 20 pages per variant at
10 fields and 10 at 100. Counted correctness on both variants, as in stage 1.

**Decision rule.** Adopt variant 2 when its bytes per unrelated write are
lower than variant 1's by a meaningful difference, no time measure is
meaningfully slower, and every correctness check passes. A meaningful
difference is larger than 5% and than both interquartile ranges, as in
stage 1. Adoption then means implementing the change in the store with its
unit tests passing; the lab result alone changes no product code.

### Host and tools

The same machine and tools as the component authoring lab: Node 24, Chromium
bundled with the package's pinned Playwright, headless, isolated pages. Host
details and background load are recorded with each run set; runs wait for a
quiet machine where possible.

### Budget and authorization

No network use and no external services. The runner needs host access to
launch Chromium.

## Conditions

| Item | Value |
| --- | --- |
| Dates | 2026-10-24: timing 18:45–18:50 UTC, counted 18:50, memory 18:50–18:52 |
| Host | macOS 15 (Darwin 24.6), x86-64, Intel Xeon W-2140B at 3.2 GHz with 16 threads, 64 GB memory, on mains power |
| Background load | 1-minute load average 6 at the start of the timing set, rising to 49 by its end; 38 to 46 during the counted and memory sets |
| Runtimes | Node 24.12.0; Chromium 156.0.8078.4 bundled with Playwright 1.64.1, new headless mode, 800 × 600 viewport; every timing page cross-origin isolated |
| Measured source | Repository commit `fc5b2b7c6c23` with uncommitted sources, identical in all three sets. SHA-256 prefixes: `store.js` `31a6a0ac8331`, `shallow.js` `f6c84d08b867`, `elements.js` `a9e42497fb74`, example `filters.js` `71c7edef2729`, `bindings.js` `3c6ccda2d7c8`, `app.js` `042c60548771`, `filter-field.js` `22a9c97ead3f`, `filter-summary.js` `7cb0d8246ef6`, control `1b1fbde01a17`, page API `8abcc86873cc`, runner `16af032e9cec` |
| Samples | Timing: 20 pages per variant at 10 fields, 10 at 1 and 100. Counted: 3 per variant and size. Memory and allocation: 10 per variant and size. No page errored |

## Results

Medians across pages of each page's median. "Meaningful" applies the plan's
rule: larger than 5% of the control and than both interquartile ranges.

### Interaction cost

| Fields | Hand-written, ms per interaction | Lattice | Difference | Meaningful |
| --- | --- | --- | --- | --- |
| 1 | 1.77 (IQR 0.066) | 1.83 (IQR 0.058) | +0.056, +3.2% | No |
| 10 | 2.29 (IQR 0.090) | 2.36 (IQR 0.133) | +0.071, +3.1% | No |
| 100 | 6.92 (IQR 0.334) | 7.21 (IQR 0.447) | +0.286, +4.1% | No |

Script duration within those interactions:

| Fields | Hand-written, ms | Lattice | Difference | Meaningful |
| --- | --- | --- | --- | --- |
| 1 | 0.060 | 0.081 | +0.021, +36% | Yes |
| 10 | 0.095 | 0.116 | +0.021, +22% | Yes |
| 100 | 0.332 | 0.385 | +0.053, +16% | Yes |

Style recalculation and layout were the same in both, within 2.4% and never
meaningful.

### Setup and teardown

| Fields | Setup, µs per field: hand-written | Lattice | Teardown: hand-written | Lattice |
| --- | --- | --- | --- | --- |
| 1 | 21.4 | 23.4 (+9%) | 13.1 | 15.7 (+20%) |
| 10 | 15.8 | 20.6 (+30%) | 7.0 | 9.8 (+39%) |
| 100 | 14.6 | 18.7 (+28%) | 6.6 | 9.1 (+40%) |

All differences are meaningful. Lattice adds about 4 to 5 µs of setup and
2.6 µs of teardown per field.

### Unchanged and unrelated updates

| Fields | Unchanged, µs per action: hand-written | Lattice | Unrelated: hand-written | Lattice |
| --- | --- | --- | --- | --- |
| 1 | 0.010 | 0.075 | 0.005 | 0.37 |
| 10 | 0.010 | 0.075 | 0.005 | 0.53 |
| 100 | 0.010 | 0.075 | 0.000 | 2.64 |

All Lattice differences are meaningful. An unchanged write costs a constant
65 ns more: the store compares the patch and returns. An unrelated write costs
more as subscriptions grow, from 0.37 µs with 3 subscriptions to 2.64 µs with
201, because the commit walks every subscription to find that none declared
the changed field. Neither variant made a DOM write for either action.

### Allocations and memory

| Fields | Measure | Hand-written | Lattice |
| --- | --- | --- | --- |
| 1, 10, 100 | Bytes per unchanged action | 2.4, 2.4, 2.6 | 26, 27, 23 |
| 1, 10, 100 | Bytes per unrelated action | 4.3, 4.3, 4.3 | 740, 1,798, 2,548 |
| 1, 10, 100 | Bytes per interaction, including the runner's own page calls | 3,129, 3,298, 5,164 | 3,774, 3,940, 3,380 |
| 100 | JavaScript heap per mounted field, bytes | 372 (IQR 59) | 755 (IQR 0) |
| 1, 10, 100 | Retained after 20 mount and teardown cycles, bytes | 16,848, 9,848, 9,572 | 44,016, 19,992, 18,100 |
| 1, 10, 100 | Listeners and nodes retained after those cycles | 0 | 0 |

Allocation figures are estimates from sampling at a 64-byte interval. The
memory per field at 1 and 10 fields is dominated by one-time costs and varied
widely for the control, with interquartile ranges up to 5,800 bytes, so only
the figure at 100 fields is read: Lattice holds about 380 bytes more per field,
the binding's internal controller, combined signal, `present` function and two
subscription records. Memory retained after churn did not grow with the number
of fields in either variant, and no listener or node remained, so neither
variant leaks per field; Lattice's larger constant is not explained by this
lab. At 100 fields the hand-written variant allocated more per interaction than
Lattice; the cause was not investigated.

### Counts

Every DOM effect was identical in both variants at every size: per typing
interaction, N field `value` assignments, N − 1 control writes, one summary
write; per lock, N control `disabled` writes and N attribute writes; none for
unchanged and unrelated actions. Both variants show the reflection echo: 2N
calls to the field's `disabled` setter per lock, half of them writing nothing.
The only difference in work is selection evaluation: Lattice calls N + 1
`read`s per typing interaction and N per lock, where the control derives the
summary once.

### Stage 2: targeted store (2026-10-24)

Variant 1 is the store as measured in stage 1; variant 2 is the targeted copy.
Allocation pages ran 19:05–19:08 UTC, timing 19:08–19:12, counted 19:12, with
the same host and tools; background load stayed high. The lab copy passed all
48 of the store's unit tests before the runs.

| Measure | Variant 1 | Variant 2 | Difference | Meaningful |
| --- | --- | --- | --- | --- |
| Bytes per unrelated write, 10 and 100 fields | 352, 351 | 98, 98 | −72% | Yes |
| Bytes per changing write, 10 and 100 fields | 364, 364 | 110, 110 | −70% | Yes |
| Bytes per unchanged write, 10 and 100 fields | 17, 17 | 17, 17 | +2% | No |
| µs per unrelated write, 10 fields | 0.53 (IQR 0.015) | 0.42 (IQR 0.010) | −22% | Yes |
| µs per unrelated write, 100 fields | 2.68 (IQR 0.18) | 2.29 (IQR 0.12) | −15% | Yes |
| µs per unchanged write | 0.075 | 0.075 | 0% | No |
| Interaction task time, ms, 10 and 100 fields | 2.52, 7.45 | 2.51, 7.18 | −0.4%, −3.6% | No |
| Interaction script time, ms, 10 and 100 fields | 0.123, 0.388 | 0.120, 0.376 | −3% | No |
| Setup and teardown, µs per field | 41–60, 15–16 | 19–21, 9–10 | Lower | No: interquartile ranges up to 50 µs |

Bytes are steady-state figures after 20,000 warmup calls, from 10 pages per
variant and size; timings from 20 pages at 10 fields and 10 at 100. Setup and
teardown varied far more than in stage 1, which is attributed to background
load; they are not read. Both variants passed every counted check at 10 and
100 fields.

### Retained memory explained (2026-10-24)

Heap snapshots before and after churn at 10 fields, 3 pages per variant,
grouped by node type and constructor, with the same churn as stage 1, repeated
with 20 and 60 cycles:

| Retained after churn | Hand-written, 20 / 60 cycles | Lattice, 20 / 60 cycles |
| --- | --- | --- |
| Application-level objects: plain objects, arrays, closures, hidden classes | 2,376 / 2,376 bytes | 3,868 / 3,868 bytes |
| Compiled code (`InstructionStream`, `Code`, bytecode, feedback vectors) | about 23 / 23 KB | about 34 / 39 KB |
| Paint timing entries (`LargestContentfulPaint`, `InteractionContentfulPaint`) | one of each per cycle | one of each per cycle |
| Blink hash table and vector backings | grown capacity, no added entries | grown capacity, no added entries |
| Stores, subscriptions, signals, controllers, elements, observers, listeners | none | none |

Lattice's extra retention after churn is one-time engine state, not objects it
keeps: more of its functions are compiled and optimized during churn, and they
add hidden classes and some grown Blink table capacity, such as the
context-lifecycle observer list that `AbortSignal.any` uses. Application-level
retention is identical after 20 and 60 cycles, so nothing grows with
mounting. The only per-cycle growth, paint timing entries, comes from the
browser's performance timeline and is the same in both variants. No change is
needed.

## Correctness

| Variant | Counted pages | Failed checks |
| --- | --- | --- |
| 0. Hand-written wiring | 9 | None |
| 1. Lattice | 9 | None |

Both passed initial values, typing, locking, unchanged and unrelated actions,
teardown, and the expected count of every effect, at 1, 10 and 100 fields.
Teardown was checked as: observers disconnected, no assignment or action call
after the abort, and DOM listener and node counts back to their baseline. It
does not cover JavaScript heap retention; see the retained-memory finding. The smoke pass
found no runner defect.

## Threats to validity

- The example is small: two selections per field and one for the summary. An
  application with selections over many fields, or much larger groups, changes
  the commit walk's share.
- The control is written for this lab; a different hand-written design could
  cost more or less.
- Headless Chromium on one shared machine. Background load rose from 6 to 49
  during the timing set and stayed near 40 for the others; interquartile
  ranges stayed small, but absolute figures may be inflated.
- Allocation sampling estimates bytes; small per-action figures, such as a few
  bytes per call, are near its resolution.
- The comparison is one example; the unrelated-update cost depends on how many
  subscriptions a store has, which an application can make much larger.

## Decision and follow-ups

**Outcome against the plan's reading.** Interaction cost shows no measurable
overhead: +3 to +4%, within run-to-run variation at every size. Lattice's own
script work per interaction is material but small, about 21 µs at 1 and 10
fields and 53 µs at 100. Rendering work is identical. Setup, teardown,
unchanged and unrelated updates, and memory per field carry material relative
overhead at small absolute costs. Both variants released every observer,
listener and node checked.

Where the overhead is, in order of size relative to the work it serves:

1. **Unrelated updates scale with subscription count.** A write no element
   depends on walks every subscription, where the hand-written wiring does
   nothing. Its allocations of about 0.7 to 2.5 KB were measured before the
   code was optimized; stage 2 found about 350 bytes in steady state, none of
   it from the walk. This is the
   in-order walk the state management contract chose first, and its cost model
   names a field index as the alternative once evidence shows the walk
   dominates. This is that evidence for unrelated writes; whether it matters
   depends on how often an application writes fields few subscriptions read.
2. **Binding setup and memory.** About 4 to 5 µs and 380 bytes per field
   binding, plus 2.6 µs of teardown, for the internal controller, combined
   signal, `present` and subscription records the bindings contract requires
   for partial-failure rollback. It matters in collections that mount and
   unmount many items.
3. **Unchanged writes.** A constant 65 ns and about 25 bytes per call; not a
   concern at the pace of user actions.

**Owner's decision (2026-10-24):**

- **Keep the current design.**
- **Allocation investigation next.** First attribute the allocations of
  unchanged and unrelated writes: an unrelated write also builds a snapshot and
  a commit record, so the subscription walk is not assumed to cause them all.
  Then compare a targeted change against the current implementation before
  considering a field index.
- **Cheaper binding setup deferred** until a representative collection
  workload shows a meaningful cost. Rollback and cleanup guarantees are not
  weakened to save a controller.
- **Retained memory, explained.** Lattice retained 10 to 27 KB more than the
  control after churn, constant in the number of fields. Heap snapshots
  attribute it to one-time engine state, compiled code and hidden classes for
  its functions, with no store, subscription, signal or element retained and
  no growth from 20 to 60 cycles; see
  [retained memory explained](#retained-memory-explained-2026-10-24).
- No binding helper is proposed from this result: the overhead is in what each
  binding allocates, not in how it is written.

**Stage 2 outcome (2026-10-24).** Under the stage's rule, variant 2 is adopted:
allocations of a changing write fell by about 254 bytes, 70 to 72%, the time
of an unrelated write fell by 15 to 22%, and no time measure got meaningfully
slower. The change is implemented in `src/modules/store/store.js`, where the
store's unit tests, the root import test and the filters example's browser
tests pass, and
[store architecture](../specs/architecture/store.md#measured-choices) records
it. The walk's time still grows with the number of subscriptions; a field
index remains unconsidered until a workload shows that walk matters.
