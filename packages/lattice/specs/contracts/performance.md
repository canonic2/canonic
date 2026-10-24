# Performance and evidence

## Purpose and status

Lattice is judged by the work it performs, not by how small its code is or how
fast its tools run. This contract defines what is measured, under which
workloads, how optimizations are compared with simpler alternatives, and how
results are recorded. The cost sections of [state management](state-management.md#cost-model),
[bindings](bindings.md#cost) and [DOM updates](dom-updates.md#cost-and-evidence)
describe the expected cost of each layer; this topic defines how those
expectations are established and kept.

**Status:** agreed contract. No capability is implemented, so no measurements
exist yet.

## Principles

- **Total work decides.** A design is better when it does less work for the
  intended workload: fewer functions called, fewer allocations, fewer DOM writes
  and layouts, and less time. One DOM write is a good outcome; the stronger
  target is doing little unnecessary work to discover that one write.
- **Selectivity has a cost.** Whatever decides what to skip, such as declared
  fields, indexes, comparisons, caches or scheduling, is work too. It must cost
  less than the work it avoids in the intended workload. For small stores and
  short lists, the simplest path often wins.
- **Simplest first.** Each capability starts with the simplest implementation
  that meets its contract. Added bookkeeping needs measurements that show it
  saves more than it costs, in a workload that matters.
- **Correctness before speed.** No optimization may weaken ordering, at-most-once
  delivery, cleanup or interaction preservation. An optimization that changes an
  observable guarantee is a contract change, not a performance change.
- **Claims need evidence.** A statement that something is faster, smaller or
  cheaper names its measure, workload and environment. Tool speed, bundle size
  of an empty package or reasoning alone are not evidence.

## Measures

### Kinds of evidence

Every number states how it was obtained. Three kinds are kept apart and never
presented as one another:

| Kind | What it is | Limits |
| --- | --- | --- |
| Observed counts | Events counted directly in a test: calls through a wrapper or spy, mutation records, resources acquired and released, identities compared | Count only what the test instruments; deterministic |
| Inferred costs | Work derived from the algorithm and the workload, such as field checks equal to subscriptions times declared fields | Not measured; valid only while the implementation matches the description |
| Profiler measurements | Allocation sampling, heap snapshots, CPU profiles and traces in a named environment | Vary between runs and machines; reported with method and variation |

Fewer counted operations is evidence to investigate, not proof that an
implementation is faster. A claim of speed needs a timing or profiler
measurement as well.

### Work counts

Observed counts are deterministic: the same input produces the same count on any
machine. They are the basis of regression tests. Each counter names its method:

| Layer | Counter | Method |
| --- | --- | --- |
| State management | `read`, `equals` and `run` calls | Observed: wrappers around the selection's functions and the `run` callback |
| State management | Snapshots per changing write | Observed: the identity returned by `get()` before and after each write |
| State management | Commits and deliveries per action | Observed: `run` calls of a subscription to every field |
| State management | Field checks per commit | Inferred from subscriptions and declared fields; not counted |
| Bindings | Input assignments, action calls | Observed: spies on the element's setters and the capability's actions |
| Bindings, element lifecycle | Listeners, observers, timers and subscriptions alive | Observed: spies on the acquiring and releasing APIs in the test |
| DOM updates | Write attempts | Observed: spies on the property setters the element writes, as [DOM updates](dom-updates.md#cost-and-evidence) describes |
| DOM updates | Mutations by type, nodes created, removed and moved | Observed: `MutationObserver` records and node identity checks |
| DOM updates | Update function calls | Observed: wrappers around the element's update functions in tests |

The runtime has no instrumentation hooks; counting lives entirely in tests. A
counter not listed with a method is not claimed.

### Allocations

Allocation matters on paths that run for every commit or every update. Tests
assert what identity shows: no new snapshot for an unchanged write, the same
reference for an unchanged field, the same node for a retained key. Identity
does not show temporary objects created and discarded along the way; a claim
about those needs allocation sampling or heap measurement in a benchmark, in a
named environment, reported with its method. Such measurements are not unit
assertions.

### Rendering and responsiveness

Fast callbacks and no forced layouts do not by themselves make feedback fast.
Rendering work and visible latency are measured in Chromium, in a named
environment:

- **Rendering work:** style recalculation, layout, paint, raster and
  compositing, from performance metrics or a trace.
- **Responsiveness:** for a real input event, three moments are distinguished
  and reported separately:

  | Moment | Evidence |
  | --- | --- |
  | Handler finished | The event's processing end, from the Event Timing API |
  | Paint opportunity | The next animation frame after processing |
  | Presented | The frame containing the update shown on screen, from the Event Timing entry's duration or a trace's presentation event |

  Only the presented moment shows what a person experiences. Callback timings
  and frame callbacks are reported as such, not as visible latency.

A test may assert that an update causes no forced synchronous layout when the
measurement is reliable in the test runner; otherwise the check is a recorded
benchmark.

### Time

Timings complement counts; they never replace them. A timing:

- runs in a named environment: hardware, operating system, Node or Chromium
  version, and power state where relevant;
- reports the sample count, the median, a high percentile and a measure of
  variation, not a single run or the minimum;
- compares variants in the same run, on the same machine, in alternating order.

Steady-state and lifetime costs are measured separately:

- **Steady state:** warmed up, with setup and teardown excluded, for the cost of
  one more update.
- **Cold and setup:** first import, first construction, first subscription and
  building any index, cache or precomputed structure, measured without warm-up.
- **Teardown:** disposal and the release of what setup built.
- **Complete lifetime:** create, then a stated number of updates, then dispose,
  at update counts representative of how long the object lives, such as a
  short-lived dialog and a long-lived panel. A mechanism that only pays off
  after many updates reports the break-even count.

## Workloads

Every performance claim names one of these workloads, or adds a new one here
with a reason:

| Workload | Shape |
| --- | --- |
| Small store | A handful of fields and a few subscriptions, single-field commits at the pace of user actions |
| Wide fan-out | Many subscriptions on one store, each depending on one or two fields, single-field commits |
| Burst | One action changing several fields, and several actions in quick succession, such as a response arriving while a person types |
| Many elements | Many instances of a small element connected, updated once each, disconnected and reconnected |
| Collection | A keyed list updated by append, removal, single-item change and reorder, at sizes typical of an application's lists |
| Frequent values | Values that change every animation frame, which must stay outside stores; used to verify they do |
| Unchanged and unrelated writes | Writes whose values equal the current ones, and commits to fields no subscription reads: the cases selectivity should make nearly free |
| No subscribers | Writes and batches on a store nobody watches |
| Cold start | Importing an entry point, constructing the first element of a class, creating the first store and subscription |
| Churn | Repeated create, update and dispose cycles of stores, bindings, elements and collection items, checking retained memory and work left running while idle |
| Interaction | A real input event, such as a click or key press, through action, commit, binding and element to the presented frame |

Workload sizes are stated with each measurement. Sizes typical of the consuming
applications are preferred over extremes; an extreme size is reported as such.

## Comparisons

- An optimization is measured against the simpler path it replaces, on the same
  workloads, including cold start, churn and complete lifetimes, not only the
  steady state.
- Before measuring, the comparison states:
  - its primary metric, such as presented latency in the interaction workload
    or `read` calls in the wide fan-out workload;
  - its secondary metrics, such as allocations, setup time or tail latency, and
    the regression it accepts on each;
  - the smallest difference it treats as meaningful, given the measurement's
    variation.
- Results report sample counts, variation, and absolute as well as relative
  differences. A difference smaller than the stated threshold, or than the
  observed variation, is not a win.
- Each comparison ends in one of three outcomes:
  - **Adopt:** the optimization improves the primary metric beyond the
    threshold and no secondary metric regresses beyond what was accepted.
  - **Reject:** it does not improve the primary metric, or a secondary metric
    regresses beyond what was accepted.
  - **Inconclusive:** the results are within noise or conflict in ways the
    stated rule does not settle. The simpler implementation is kept.
- Results where the simpler path wins are recorded as well. They define where an
  optimization should not be applied, such as a field index on a small store.
- Variants exist only in benchmark code. The shipped runtime contains one
  implementation; there are no runtime switches between strategies.
- The baselines that matter today:

  | Mechanism | Baseline it must beat |
  | --- | --- |
  | Declared-field skipping | Calling every subscription's `read` and comparing |
  | A field index for subscriptions | The in-order walk with a field check per subscription |
  | Shallow freezing | Unfrozen snapshots, to decide whether freezing's cost is material |
  | A keyed collection helper | A straightforward hand-written keyed update |
  | Any scheduling | Synchronous application of the same updates |

## Regression protection

- Work counts for each capability's contract are asserted in its unit or browser
  tests, as each topic's testing section lists. A change that raises a count
  fails a test and must justify the new count in review.
- Timings are not asserted in the test suites, because they vary between
  machines and runs. Benchmarks run on request, separately from tests.
- Benchmarks are files named `*.bench.js` beside the module they measure, run by
  the package's test runner in benchmark mode. They import the shipped modules
  unchanged. The package has no benchmark command yet; the change that adds the
  first benchmark also adds its configuration and a documented command to the
  package README.
- Fewer counted operations is not proof of a faster implementation; count
  regressions are investigated, and speed claims rest on timings or profiles.

## Recording evidence

Measurements that inform a decision are recorded in the spec that owns the
decision, under a dated evidence entry with:

- the question being answered and the variants compared;
- the primary metric, secondary metrics, accepted regressions and threshold,
  stated before measuring;
- the workload, its sizes and update counts;
- the environment;
- the kind of evidence for each number: observed, inferred or profiler;
- the command or benchmark file that produced the result;
- the results with sample counts and variation, including runs where the simpler
  path won;
- the outcome, adopt, reject or inconclusive, and the decision it supports.

Raw output stays out of the specs; a summary table and the means to reproduce it
are enough. A recorded result is superseded by a newer one rather than edited.

## Decisions

- **Counts over timings for regressions.** Deterministic counts make a test fail
  for the right reason on every machine; timings stay in benchmarks where their
  variance can be handled.
- **No runtime instrumentation.** Counting in tests keeps production paths free
  of hooks and conditionals that would themselves cost work.
- **One shipped strategy.** Alternatives are compared in benchmarks, not kept
  behind flags, so the runtime stays small and each path is fully tested.
- **Rules before results.** Naming the primary metric, accepted regressions and
  threshold before measuring keeps a comparison from being decided by whichever
  number looks best afterwards. When results are inconclusive, simplicity wins.
- **Lifetimes, not just steady state.** Indexes, caches and precomputed
  structures pay for construction and teardown; comparing complete lifetimes at
  realistic update counts keeps them from looking cheaper than they are for
  short-lived objects.

## Open questions

- Which consuming workloads and sizes become the reference set once the first
  application adopts Lattice.
- Whether the browser test runner exposes Chromium's layout and style counts
  reliably enough to assert "no forced layout" in tests rather than in recorded
  benchmarks.
