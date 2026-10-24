# Component authoring shape

## Question

Which way of writing a Lattice element adds the least total work to getting a
change into the right element input and carrying it through the element's
lifecycle: a native class written by hand, a native class with narrow helpers,
or a setup function that runs once, with Lattice keeping one internal class per
definition?

Elements are Web Components that own their DOM, styles and presentation
updates. Lattice supplies input plumbing, lifecycle mechanics and, later, state
delivery; it does not render an element's tree. The comparison therefore keeps
each element's presentation code identical across variants and measures the
machinery around it: input dispatch, equality checks, early-input handling,
event routing, connection resources, allocations and cleanup. Browser rendering
is measured as a guard that this machinery causes no extra presentation work,
not as the subject of the experiment.

The answer decides the authoring shape that
[element lifecycle](../specs/contracts/element-lifecycle.md) specifies before the
framework guide on elements is written. Total work decides: JavaScript
bookkeeping, DOM changes, browser rendering, memory and idle activity, for the
same correct, visible behavior. How pleasant a shape is to write does not.

The real store does not exist yet. This is a component-authoring comparison
through a stand-in application harness; the integrated flow through the real
store and bindings is confirmed in a later stage of this record.

## Status

Decided, 2026-10-24. The owner agreed to the plan on 2026-10-24, including the
long-lived workload at 10 elements as the primary cell. Under the decision
rule, the class with helpers is adopted and both setup-function shapes are
rejected. The owner chose the class with helpers on 2026-10-24, without the
confirmation run.

## Hypothesis

- The variants share the same presentation code, so for one interaction they
  should differ only in the work between an input assignment and the
  presentation call: an accessor, an equality check and a call to the update
  function. That work is expected to be a small share of an interaction's total
  main-thread work, so differences in interaction cost are expected to fall
  within 5% of the control, and possibly within noise.
- Memory per instance is where the variants are expected to diverge. A setup
  function returning one closure per input allocates those closures, and the
  object holding them, for every instance. A native class keeps its methods on
  the prototype. Shared update functions receiving the instance's node
  references avoid per-instance closures without a class written by the author.
- No variant is expected to add idle work. Each acquires one `ResizeObserver`
  per connection and nothing else.

This is reasoning, not evidence; no exploratory measurement was taken.

## Method

### Element

Every variant implements the same element, `<acme-filter-field>`, and must meet
the whole element lifecycle contract.

| Part | Behavior |
| --- | --- |
| Shadow tree | A `<label>` holding a text node and an `<input type="search">`, built once by cloning a template created on the first construction |
| Stylesheet | One constructed stylesheet created on the first construction and adopted by every instance |
| `label` | String input reflected to the `label` attribute; writes the label's text node |
| `value` | String input; compared with the input control's live value, written only when they differ |
| `placeholder` | String input from the `placeholder` attribute; writes the control's `placeholder` |
| `disabled` | Boolean input reflected to the `disabled` attribute with HTML presence semantics; writes the control's `disabled` |
| Intent event | `acme-filter-input`, bubbling and composed, with `detail: { value }`, dispatched on the control's `input` event only while the host is connected |
| Connection resource | A `ResizeObserver` on the host, created per connection and released through the connection signal, toggling a `compact` class on the label |
| Move | `connectedMoveCallback` that does nothing, so `moveBefore` keeps the connection |
| Registration | `defineAcmeFilterField(registry = customElements)` with the contract's guard |

Presentation code is shared and imported by every variant: the template and
node lookup, the stylesheet, the four functions that write an input's value to
its nodes, the observer callback and the event payload. Variants differ only in
how they declare inputs, store values, decide whether to call those functions,
handle early inputs, manage the connection and register. This exclusion is by
design.

### Variants

| Variant | Role | Author writes | Lattice-side code |
| --- | --- | --- | --- |
| 0. Hand-written class | Control: the contract as it stands, with no shared machinery | A class with private fields, getters and setters per input, capture and replay, connection controller, registration guard | None |
| 1. Class with helpers | Candidate | A class with hand-written setters, calling helpers for early-input capture and replay, stylesheet adoption and registration | `captureInputs(host, names)` and `replayInputs(host, pending)`, `adoptSheet(root, factory)`, `defineOnce(registry, name, constructor)` |
| 2. Setup with closures | Candidate | `element({ inputs, attributes, styles, setup(host, root) })`, where `setup` returns one update function per input that closes over node references | One generated class per definition: accessors installed once on its prototype, values in a per-instance array indexed by input, capture and replay, attribute mapping, connection controller when `connected(host, signal)` is declared, registration guard |
| 3. Setup with shared handlers | Candidate | `element({ inputs, attributes, styles, setup(host, root), update: { label(refs, value) } })`, where `setup` returns an object of node references and `update` holds one function per input, shared by every instance | The same generated class as variant 2, calling `update[input](refs, value)` instead of per-instance closures |
| 4. Bare element | Diagnostic floor, never adopted | A class that builds the same shadow tree and stylesheet, with no inputs, events or connection work | None |

Framework overhead is each candidate's difference from variant 0, which is
functionally equivalent. The bare element omits required features, so its
difference from any other variant includes those features as well as the
authoring machinery; it only shows how much of the total the shared shadow tree
and stylesheet cost, and takes part in lifecycle and memory cells only.

Every variant is implemented as efficiently as it reasonably can be, with the
same care; there is one implementation per variant and no tuning after results
exist. All variants skip equal values with `Object.is`, except `value`, which
compares with the control's live value.

### Application harness

One harness, identical for every variant, stands in for state management and
bindings:

- a plain state object, `{ query, disabled, labels, revision }`, with one
  synchronous action per change and a list of subscribers called after each
  action;
- one binding per element that assigns `value`, `disabled` and `label` from the
  state when the corresponding field changed, and calls the `setQuery` action
  from the `acme-filter-input` event;
- a plain button outside the elements that calls `toggleDisabled`, and an
  unrelated action, `bumpRevision`, that changes a field no binding reads.

The harness counts its own work: actions, subscriber calls and input
assignments.

### Workloads

| Workload | Sequence |
| --- | --- |
| Typing | A real key press into the first element's control through Playwright's keyboard: native `input` event, intent event, `setQuery`, the binding assigns `value` to every element of the group; the typed element's live value already matches, the others are written |
| Toggle | A real click on the harness button: `toggleDisabled`, the binding assigns `disabled` to every element, which reflects it |
| Unchanged | An action that sets the same `query` and `disabled` again, so every binding assigns the current values |
| Unrelated | `bumpRevision`: no binding reads the field, so nothing should be assigned |
| Long-lived | Elements created and connected once, then 100 interactions alternating typing and toggle, each in its own frame |
| Short-lived | Repeated mounting: create and connect the group, one typing interaction, a frame, remove the group; 20 cycles |
| Lifecycle throughput | Create, connect, assign changed inputs 10 times and remove the group synchronously, with no frame in between; a secondary stress cell, since the browser may skip work for states it never displays |
| Lifecycle steps | Definition and first construction (cold start), construction, early-input upgrade, connection, reconnection and moves with `moveBefore`, each timed separately |
| Idle | Connected group, settled, then 2 seconds with no interaction |

Every interactive workload waits for the next frame between interactions, so
each update is rendered.

### Sizes

| Size | Meaning |
| --- | --- |
| 1 element | A single control |
| 10 elements | A small group, the representative size |
| 100 elements | A typical list |
| 1,000 elements | Stress, reported as such; never decides the shape alone |

### Measures

| Measure | What it shows | Evidence |
| --- | --- | --- |
| Interaction cost | Main-thread work per interaction, including microtasks, observer callbacks, style, layout and paint that the interaction causes, wherever they run | Profiler: deltas of Chrome DevTools Protocol `Performance.getMetrics` (`TaskDuration`, `ScriptDuration`, `RecalcStyleDuration`, `RecalcStyleCount`, `LayoutDuration`, `LayoutCount`) across a sequence of interactions, divided by their number |
| Responsiveness, DOM completion | Time from the input event to the last targeted DOM write | Observed timing: `event.timeStamp` of the native `input` event to `performance.now()` at the end of the harness's synchronous delivery |
| Responsiveness, presentation | Time from the input to the presented frame | Profiler: `EventTiming` events in a trace, in the traced run set only; Event Timing entries in the page do not report interactions under 16 ms, so they are not used. If the trace lacks the event in this Chromium, the record says so and reports the paint opportunity, the next animation frame, labeled as such |
| Unchanged and unrelated updates | Work avoided, and the bookkeeping spent avoiding it | Observed: assignments, accessor calls, equality checks, presentation calls and write attempts, which should be zero for unchanged values; plus interaction cost for these workloads |
| Write attempts | Writes the element tried to make | Observed: spies on the control's `value`, `placeholder` and `disabled` setters, `Text.data`, `setAttribute`, `removeAttribute`, `toggleAttribute` and `classList` |
| Mutations | Changes the DOM recorded | Observed: `MutationObserver` records on the host and its shadow tree, counted separately from write attempts |
| Lifecycle cost | Definition, construction, early-input upgrade, connection, reconnection, moves and removal | Observed timing with `performance.now()` around each synchronous step, plus interaction cost for the short-lived workload |
| Memory | Retained memory per instance and retention after teardown | Profiler: `HeapProfiler.collectGarbage`, then `Runtime.getHeapUsage`, with the group connected and after 10 short-lived cycles |
| Idle impact | Work running with no interaction | Observed: counts of timers, animation frames, observer callbacks and listeners added, from spies; profiler: `TaskDuration` delta over the idle period |

### Run sets

Instrumentation changes what it measures, so three kinds of page are kept apart:

| Run set | Contains | Excludes |
| --- | --- | --- |
| Clean timing | Interaction cost, responsiveness to DOM completion, lifecycle steps, memory, idle `TaskDuration` | Spies, mutation observers, tracing |
| Counted | Write attempts, mutations, harness counts, presentation calls, idle callback counts, correctness checks | Timings, which are discarded |
| Traced | Presentation timing and a sample of full traces for inspection | Other timings |

- Each sample is a fresh browser context and page that loads exactly one
  variant, so no variant's compiled code or object shapes affect another's.
- Variants alternate in an order shuffled with seed 1. Clean timing gets 20
  pages per variant and size, counted runs 3, traced runs 5, memory 10.
- Within a clean timing page: cold start first, without warmup; then each
  workload with 5 warmup repetitions and 10 measured repetitions, in a fixed
  order that is the same for every variant.
- Files are served to the page through Playwright request routing from the lab
  folder, so no network listener or external request is involved.

### Inputs

A deterministic generator in the lab, seeded with 1, produces labels such as
`Filter 0042`, placeholders such as `Search acme 0042`, and the typed text from
a fixed word list. Every variant receives identical sequences.

### Correctness checks

Run in every counted page, on fresh elements, for every variant except the bare
diagnostic, which only checks markup and stylesheet:

| Check | Pass condition |
| --- | --- |
| Import | Importing the module defines nothing and creates no stylesheet |
| Registration | Defining twice with the same definition succeeds; a different constructor under the same name throws |
| Constructor safety | `document.createElement` succeeds, which the platform only allows when the constructor adds no attributes or children to the host |
| Early inputs | A pre-upgrade property wins over the attribute, the attribute reflects it, and the getter returns it before the replay runs |
| Assignment during replay | An assignment after upgrade and before the replay keeps the newer value |
| No capture | An element upgraded with no early properties queues no replay |
| Targeted update | Each workload produces exactly the expected presentation calls, write attempts and mutations, decided before any run; the same in every variant |
| Unchanged and unrelated | Zero presentation calls, zero write attempts and zero mutations |
| Live value | After typing, assigning the previous property value writes the control; assigning the typed value writes nothing |
| Disconnected intent | An `input` event on a disconnected element's control dispatches no event |
| Connection resources | Observe and disconnect counts, from spies on `ResizeObserver`, return to zero after removal and stay at one per element after several reconnections |
| Move | `moveBefore` keeps the observer: no new observe or disconnect |
| Idle | No timer, animation frame or listener is added while idle, and no observer callback runs after settling |
| Output | After a scripted sequence, the shadow markup and host attributes equal an expected snapshot written by hand before any run |

A variant that fails any check in any page is rejected, whatever its timings.

### Metrics and decision rule

- **Primary metric:** interaction cost, the main-thread `TaskDuration` per
  interaction, in the long-lived workload at 10 elements, median across pages.
- **Constraints**, which a candidate must meet against the control:
  - interaction cost at 1 element and in the short-lived workload at 10
    elements: no regression beyond 5%;
  - memory per instance: no regression beyond 10%;
  - retained memory after teardown: no growth beyond run-to-run variation;
  - idle impact: no work while idle, as the correctness check requires.
- **Secondary metrics**, with the regression each may show against the
  control: responsiveness to DOM completion, 5%; lifecycle steps, 10%; cold
  start, 20%; lifecycle throughput, 10%; interaction cost at 100 and 1,000
  elements, 10%; presentation latency, within run-to-run variation.
- **Meaningful difference:** 5% of the control's median, and larger than the
  interquartile range of the pages' medians for both variants.
- **Outcome for each candidate:**
  - adopt when it matches or beats the control on the primary metric within
    the threshold and meets every constraint and secondary limit;
  - reject when it is slower than the control by more than the threshold on the
    primary metric, or breaks a constraint or secondary limit;
  - inconclusive when differences are within noise.
- **Choice between adoptable candidates:** the lower interaction cost. When
  they are within noise of each other, the one with less memory per instance;
  when that is also within noise, the owner decides.
- A shape that finishes a synchronous cell faster by moving work outside the
  timer does not win: interaction cost counts all main-thread work the
  interaction causes, and idle and frame work are measured separately.

### Amendment before the first run (2026-10-24)

Settled while building the lab, before any run:

- **Repetitions.** Ten measured repetitions of 100 interactions per page would
  take several hours on a shared machine. Each interactive workload instead
  measures blocks within one page:

  | Workload | Unit | Warmup | Measured |
  | --- | --- | --- | --- |
  | Long-lived | A block of 7 cycles, each a key press into the first element, a click that disables the group and a click that enables it: 21 interactions, with a frame after each | 1 block | 5 blocks |
  | Short-lived | A block of 5 cycles: mount, focus, one key press, frame, unmount, frame | 1 block | 4 blocks |
  | Unchanged, unrelated | A block of 10 actions, each followed by a frame | 1 block | 2 blocks |
  | Lifecycle steps, lifecycle throughput | One repetition | 5 | 10 |

  Clean timing uses 20 pages per variant at 10 elements and 10 pages at the
  other sizes. Each block's metric deltas are divided by its number of
  interactions or cycles.
- **Focus.** Clicking the toggle button moves focus to it, so each cycle
  focuses the first element's control before typing. The focus call is the
  same in every variant.
- **Early-input upgrade.** An element constructed after definition cannot be
  captured. The upgrade cell creates its elements before the variant's module
  is imported, sets `label` and `value` on each and a `label` attribute on half,
  then calls `customElements.upgrade` on each after definition. The correctness
  checks create unupgraded elements in an inert document and adopt them.
- **Attributes present at upgrade.** The initial `attributeChangedCallback` for
  an input captured as a property is ignored, so the property wins. It is
  recognized by a null old value while that input is pending. An attribute
  added for the first time between upgrade and replay is indistinguishable and
  also loses to the property; the contract's precedence rule does not settle
  this case.
- **Event listeners.** The native classes and variant 3 register the host
  itself as the control's listener object (`handleEvent`). Variant 2 registers
  a closure from `setup`, which is the shape's own form.
- **Heap measure.** `Runtime.getHeapUsage` reports the JavaScript heap only.
  DOM nodes live in Blink's heap and are identical across variants;
  `Memory.getDOMCounters` is recorded beside it to confirm equal node counts.
- **Expected counts**, per workload, for a group of N elements. Presentation
  calls are calls to the shared write functions; the `value` function compares
  with the control's live value before writing.

  | Workload | Assignments | Presentation calls | Write attempts | Mutations | Other |
  | --- | --- | --- | --- | --- | --- |
  | Mount | 3N | 2N | 3N: label text, `label` attribute, the observer's `classList.toggle` | 2N | N observers, N observer callbacks |
  | Typing | N | N | N − 1: every control except the typed one | 0 | |
  | Disable, enable | N each | N each | 2N each: control `disabled`, host attribute | 2N each | |
  | Unchanged | 3N | N: the `value` comparisons | 0 | 0 | |
  | Unrelated | 0 | 0 | 0 | 0 | 1 action |
  | Idle, 2 seconds | | | 0 | 0 | No timers, frames, listeners, microtasks or observer callbacks |

### Amendment after the smoke runs (2026-10-24)

Three smoke runs, one page per variant and run set at 10 elements, found two
runner defects. Both were fixed before any measured run:

- **Trace parsing.** `EventTiming` trace events reuse their async ids, so
  pairing begin and end events by id dropped most interactions: 2 of 21 were
  found per page. Each begin event carries the event's `duration` from its
  timestamp to the presented frame, so the runner now reads that directly and
  takes the longest event per interaction id. Afterwards, 20 to 21
  interactions were found per page.
- **Timer resolution.** Without cross-origin isolation, `performance.now()` is
  coarsened to 100 µs, and lifecycle cells at 10 elements measured 0 to 0.2 ms.
  Pages are now served with `Cross-Origin-Opener-Policy: same-origin` and
  `Cross-Origin-Embedder-Policy: require-corp`, which gives 5 µs resolution;
  every timing page records that it is isolated. Each lifecycle-step and
  lifecycle-throughput repetition, and the early-input upgrade, now processes
  groups of the page's size until at least 1,000 elements have gone through,
  and reports milliseconds per element.

The smoke runs found no correctness failures; every variant matched every
expected count.

### Reporting requirements (2026-10-24, before measured results)

Added after the smoke runs, without changing the decision rule:

- An interaction costs about 2.5 ms of main-thread work, mostly the browser's
  input and frame handling, while script work is about 0.1 ms. The 5%
  threshold on the primary metric is therefore larger than all script work,
  and equal interaction cost does not establish equal CPU overhead. Results
  report script duration in absolute terms and as each candidate's difference
  from variant 0, separately from interaction cost. These figures describe the
  machinery's own cost; they do not decide the outcome.
- Counts are reported as a result in their own right: whether every candidate
  makes the same presentation calls, no unnecessary writes and releases its
  connection resources.
- "No idle work" covers the 2-second observation period only.
- When timing differences are within noise, the record says so, and the choice
  rests on memory and lifecycle costs as the decision rule provides.

### Confirmation plan (2026-10-24, before the confirmation run)

The confirmation repeats variant 0 and variant 1 with the same runner, pages,
sizes, workloads and seeded order as the measured timing and memory sets,
restricted to those two variants. It confirms the result when variant 1 again
matches variant 0 on the primary metric within the threshold and meets every
constraint and secondary limit. Differences within run-to-run variation count
as matching. A failure reopens the decision.

### Stage 2: cold start with several definitions (2026-10-24, before any run)

**Question.** Variant 3 was rejected only on cold start, measured with one
element definition per page. Does its one-time cost of about 0.3 ms repeat for
each definition, or is it mostly paid once for the shared element module?

**Variants.** Variant 0, variant 1 and variant 3, unchanged. Variant 2 stays
rejected on memory and is not measured.

**Method.** Each page loads K element definitions: K copies of the variant's
module, served at distinct URLs with the tag renamed to
`acme-filter-field-<i>`, with every module preloaded as before. The cold-start
timer starts before importing all K modules together and stops after each has
been defined and constructed its first element. K is 1, 5 and 20; each
variant gets 20 pages per K, in a seeded order. Pages are cross-origin
isolated, as in the measured sets.

Copies of one module share no compiled code, because each is a separate
module record. Variant 3's generic class code lives in the shared element
module, evaluated once per page, which is the effect being measured.

**Decision rule.** The original limit applies at each K: cold start within 20%
of variant 0, compared by median with the meaningful-difference test. If
variant 3 meets it at K = 5 and K = 20, its cold-start rejection is limited to
pages with a single definition, and it becomes adoptable in multi-definition
applications. Between adoptable candidates the original rule still applies:
lower interaction cost, then lower memory per instance. Unless the owner
changes that rule before this stage runs, variant 1 remains the choice,
since its memory per added element is lower: 310 against 336 bytes.

### Host and tools

macOS on Apple silicon, on AC power, with no builds or test suites running
during measurement. Node 24 and the Chromium build bundled with the package's
pinned Playwright, headless. Host details are recorded with each run set.

### Budget and authorization

No network use and no external services. The runner needs host access to
launch Chromium.

## Conditions

| Item | Value |
| --- | --- |
| Dates | 2026-10-24: timing 15:15–15:50 UTC, counted 15:50–15:53, traced 15:53–15:55, memory 15:55–15:59 |
| Host | macOS 15 (Darwin 24.6), x86-64, Intel Xeon W-2140B at 3.2 GHz with 16 threads, 64 GB memory, a desktop on mains power. The plan named Apple silicon; the measurements ran on this machine instead |
| Background load | Other sessions shared the machine. The 1-minute load average rose from 5.5 to 14 during the run sets, and the 15-minute average reached 15 |
| Runtimes | Node 24.12.0; Chromium 156.0.8078.4 bundled with Playwright 1.64.1, new headless mode, 800 × 600 viewport |
| Pages | Cross-origin isolated in every timing page; each page a fresh browser context loading one variant |
| Measured source | Repository commit `00cff21b5e7c` with the lab's uncommitted sources, identical in all four run sets. SHA-256 prefixes: runner `9601684ffcb0`, page API `aa39d6108afd`, presentation `6be8df69b1b0`, variant 0 `93049ac3b64b`, variant 1 `d524e9a95e2d` with helpers `55f8ff536c94`, variant 2 `d15fc20ba123` with its element module `23e5db0ed8cf`, variant 3 `3114a29f83b1` with its element module `97e134203d86` |
| Samples | Timing: 20 pages per variant at 10 elements and 10 at each other size. Counted: 3 pages per variant and size. Traced: 5 pages per candidate and size. Memory: 10 pages per variant and size. No page errored |

### Confirmation and stage 2 (2026-10-24)

| Run | Time, UTC | 1-minute load average, start to end | Notes |
| --- | --- | --- | --- |
| Confirmation timing, variants 0 and 1 | 16:46–17:08 | 27 to 62 | `bench.js` gained the stage 2 method at 16:54:06, while this run was in progress. The logged page durations add up to the run's wall-clock time, which places the change during page 39 of 100: pages 1 to 38 loaded the hashed file, page 39 is ambiguous, and pages 40 to 100 loaded the changed file. The change added a method these pages never call; it changed no measured function |
| Confirmation memory, variants 0 and 1 | 17:08–17:10 | 60 to 59 | |
| Stage 2, variants 0, 1 and 3 | 17:10–17:13 | 64 to 236 | |

Same host, runtimes and isolation as the measured sets. Other sessions loaded
the machine far beyond its 16 threads during all three runs.

## Results

All values are medians across pages of each page's median. "vs 0" is the
difference of that median from variant 0. A difference counts as meaningful
only when it exceeds 5% of variant 0's median and the interquartile range of
the pages' medians for both variants, as the decision rule defines.

### Counts

Every candidate, at every size, made exactly the expected presentation calls,
write attempts and mutations in every workload: one targeted write per changed
input, none for unchanged or unrelated updates, and one observer per connected
element, released on removal. Style and layout counts per interaction were
identical across variants at every size (3.33 or 3.67 style recalculations and
1.00 or 1.33 layouts per interaction). Over the 2-second idle period, no
variant added a timer, frame, listener or microtask, and no observer callback
ran. The authoring shape changed none of the work the browser was asked to do.

### Primary metric: interaction cost, long-lived, 10 elements

| Variant | Pages | Median ms per interaction | p95 | IQR | vs 0 |
| --- | --- | --- | --- | --- | --- |
| 0. Hand-written class | 20 | 2.29 | 2.66 | 0.109 | |
| 1. Class with helpers | 20 | 2.29 | 2.57 | 0.173 | −0.1% |
| 2. Setup with closures | 20 | 2.29 | 2.58 | 0.229 | +0.3% |
| 3. Setup with shared handlers | 20 | 2.23 | 2.61 | 0.190 | −2.7% |

No candidate differs meaningfully from the control.

### Script duration

Reported as the reporting requirements ask; it does not decide the outcome.
Script work per interaction in the long-lived workload:

| Size | Variant 0, ms | Variant 1 | Variant 2 | Variant 3 |
| --- | --- | --- | --- | --- |
| 1 | 0.0674 | −4.4% | −5.1% | −3.7% |
| 10 | 0.101 | +1.8% | −2.4% | −6.0% |
| 100 | 0.362 | +3.7% | +3.9% | +5.0% |
| 1,000 | 3.02 | +1.5% | +1.0% | +4.2% |

None of these differences is meaningful. Script duration includes the harness,
event dispatch and the runner's own page calls, so this method cannot resolve
differences in the authoring machinery below a few microseconds per
interaction.

### Constraints

| Constraint | Variant 1 | Variant 2 | Variant 3 |
| --- | --- | --- | --- |
| Interaction cost, 1 element (variant 0: 1.78 ms) | −5.2%, met | −7.1%, met | −7.1%, met |
| Short-lived cycle, 10 elements (variant 0: 5.00 ms) | −1.9%, met | +0.2%, met | −0.4%, met |
| Memory per instance, within 10% | Met at every size | **Not met**: +49.6% at 100, +81.5% at 1,000 | Met: +4.0% at 100, +8.0% at 1,000 |
| Retained memory after teardown | Met, as read below | Met, as read below | Met, as read below |
| No idle work | Met | Met | Met |

Memory per instance, JavaScript heap, bytes:

| Size | 0 | 1 | 2 | 3 | 4, bare |
| --- | --- | --- | --- | --- | --- |
| 1 | 12,628 | 12,512 | 12,972 | 12,072 | 1,084 |
| 10 | 3,392 | 3,063 | 3,289 | 2,871 | 269 |
| 100 | 585 | 608 | 876 | 609 | 124 |
| 1,000 | 336 | 340 | 610 | 363 | 77 |
| Per added element, from 100 to 1,000 | 308 | 310 | 580 | 336 | 72 |

At 1 and 10 elements the figure is dominated by one-time costs, such as
compiled code and the shared template, divided by few elements; the cost of
each added element is the slope between 100 and 1,000. The setup function with
closures adds about 270 bytes per element: its context object, the object of
update functions and five closures. Shared update functions add about 28 bytes,
mostly the array that holds input values in place of private fields.
`Memory.getDOMCounters` showed the same nodes per element in every
non-bare variant: 12 per element.

Retained memory after 10 mount-and-remove cycles was between 5.7 KB and 26 KB
for every variant, including the bare element, and did not grow with the number
of elements: 1,000 elements per cycle retained no more than 1. The heap
measurement repeated exactly across pages, with an interquartile range of 0 in
almost every cell, so the constraint's "beyond run-to-run variation" cannot be
applied literally: every few-kilobyte difference between variants would exceed
a variation of zero. Read as "no retained memory that scales with the elements
created", every variant meets it. This reading is an interpretation made after
the results and is left to the owner.

### Secondary metrics

| Metric, limit | Variant 1 | Variant 2 | Variant 3 |
| --- | --- | --- | --- |
| DOM completion, 5% | Within at every size | Within at every size | Within at every size; the largest, +8.8% at 100 elements, is smaller than both IQRs |
| Lifecycle steps, 10% | Within; the largest, removal +16.1% at 1,000, is smaller than variant 1's IQR | Within | Within |
| Cold start, 20% | Within: −2.6% to +10.7% | **Exceeded**: +23.8% to +29.3% at every size | **Exceeded**: +24.5% to +27.7% at every size |
| Lifecycle throughput, 10% | Within | Within | Within |
| Interaction cost at 100 and 1,000, 10% | Within | Within | Within |
| Presentation latency, within variation | Within | Within | Within |

Cold start, import plus definition plus first construction, in milliseconds:

| Size | 0 | 1 | 2 | 3 | 4, bare |
| --- | --- | --- | --- | --- | --- |
| 1 | 1.17 | 1.14 | 1.48 | 1.47 | 1.06 |
| 10 | 1.14 | 1.18 | 1.48 | 1.43 | 1.06 |
| 100 | 1.13 | 1.19 | 1.40 | 1.44 | 1.06 |
| 1,000 | 1.12 | 1.24 | 1.44 | 1.43 | 1.02 |

The generated class costs about 0.3 ms once: evaluating the element module,
processing the definition and installing accessors. It was measured for one
definition per page; how much of it a second definition would repeat was not
measured.

Other observations, not limits in the plan:

- Unchanged updates at 1,000 elements: variant 3 took 15.2% longer than the
  control, 1.23 against 1.06 ms per action, a meaningful difference. The
  difference is about 57 ns per assignment over the 3,000 assignments per
  action; attributing it to the generic setter's pending, live-input and
  equality checks is inferred, not measured. Variants 1 and 2 were 12% and 11%
  slower there, within their variation.
- Presentation latency was one frame, about 16 ms, up to 100 elements, and
  about 44 ms at 1,000 elements, the same for every variant.
- Idle main-thread work at 1,000 elements varied between pages from 0.4 to
  4 ms per 2 seconds, in every variant, while the counted pages recorded no
  callbacks of any kind. It is attributed to engine housekeeping such as
  garbage collection and is not a variant effect.
- Pages for variant 1 at 1,000 elements showed the largest tails, for example
  a p95 interaction cost of 163 ms against a median of 44.5 ms, coinciding with
  the highest background load.

### Confirmation (2026-10-24)

Variants 0 and 1, repeated with the same method under much heavier load:

| Cell | Variant 0 | Variant 1 | vs 0 | IQR, 0 and 1 |
| --- | --- | --- | --- | --- |
| Primary: long-lived, 10 elements, ms per interaction | 2.52 | 2.57 | +1.8% | 0.218, 0.168 |
| Long-lived, 1 element | 1.95 | 1.96 | +0.6% | 0.053, 0.153 |
| Short-lived, 10 elements, ms per cycle | 5.72 | 5.64 | −1.3% | 0.401, 0.421 |
| Cold start, 10 elements, ms | 1.17 | 1.22 | +4.3% | 0.100, 0.060 |
| Memory per instance, 1,000 elements, bytes | 336 | 340 | +1.1% | 0, 0 |

The primary metric, the constraints, cold start and memory confirm the first
run: variant 1 matches variant 0, and memory repeated byte for byte. Cells at
100 and 1,000 elements and the lifecycle-step cells did not reproduce their
earlier precision. Their interquartile ranges grew to between a third of and
more than the median, for example lifecycle throughput at 10 elements, 30.7
against 80.5 µs per element with ranges of 69 and 62. None of their
differences is meaningful under the rule, but at that variation they could not
have detected a 10% regression, so for those cells the confirmation is
inconclusive rather than confirming.

Restricted to pages 1 to 38, which loaded the hashed page source, the primary
cell has 8 pages of variant 0 and 10 of variant 1: medians 2.44 and 2.57 ms,
+5.4%, with interquartile ranges of 0.20 and 0.24 ms. The difference is
smaller than both ranges, so it is not meaningful, and the conclusion is the
same as for all 100 pages.

### Stage 2: several definitions (2026-10-24)

Cold start in milliseconds, 20 pages per cell, with every definition defined
and the pages isolated:

| Definitions | Variant 0 | Variant 1 | Variant 3 | Variant 3 vs 0 |
| --- | --- | --- | --- | --- |
| 1 | 1.25 (IQR 0.055) | 1.34 (+7.6%) | 1.56 (+25.7%) | +0.31 ms, meaningful |
| 5 | 1.86 (IQR 0.090) | 1.84 (−0.8%) | 2.24 (+20.8%) | +0.38 ms, meaningful |
| 20 | 3.59 (IQR 0.250) | 3.54 (−1.5%) | 3.63 (+1.1%) | +0.04 ms, within noise |

Each added definition cost about the same in every variant: 0.12 ms for
variant 0, 0.12 ms for variant 1 and 0.11 ms for variant 3, from the
difference between 1 and 20 definitions. Variant 3's extra cost therefore does
not scale with the number of definitions. The data are consistent with a fixed
cost per page for the shared element module, but under this load they do not
establish one: the extra was 0.31 ms with 1 definition, 0.38 ms with 5 and
within noise with 20.

Against the stage's rule, variant 3 meets the 20% limit at 20 definitions and
misses it narrowly at 5, so its cold-start rejection stands for pages with a
few definitions. The load during this stage was the highest of the whole lab,
although the cold-start interquartile ranges stayed small.

## Correctness

| Variant | Counted pages | Failed checks |
| --- | --- | --- |
| 0. Hand-written class | 12 | None |
| 1. Class with helpers | 12 | None |
| 2. Setup with closures | 12 | None |
| 3. Setup with shared handlers | 12 | None |
| 4. Bare element | 12 | None; markup and stylesheet only |

Every non-bare variant passed import, registration, constructor safety, early
inputs, assignment during replay, no capture, targeted update counts,
unchanged and unrelated updates, live value, disconnected intent, connection
resources, move, idle and output checks on every counted page. The three smoke
run sets also passed every check.

## Threats to validity

- Background load from other sessions rose during the run sets, to a load
  average of about 15 on 16 threads. Medians were stable, but tails at 1,000
  elements were inflated in some cells.
- The measuring host is an Intel desktop, not the Apple silicon machine the
  plan named.
- Memory figures cover the JavaScript heap only; DOM node counts were equal.
- Cold start was measured for a single element definition per page in the
  first run; stage 2 measured 1, 5 and 20 identical definitions. Copies of one
  module share no compiled code, but real elements differ in size and shape.
- The confirmation and stage 2 ran with load averages between 17 and 236 on 16
  threads.

- The harness stands in for the real store and bindings; its delivery cost is
  the same in every variant but differs from the eventual implementation. The
  result covers component-authoring overhead under this harness, not the
  completed framework.
- Pages are cross-origin isolated for timer precision. Hosts such as editor
  webviews may run under different conditions; the lab can select a candidate,
  and integration evidence in the real host establishes its behavior there.
- The element is small. Larger elements spend more time in their own
  presentation work, which shrinks relative differences.
- Headless Chromium on one machine; the product's hosts, such as editor
  webviews, are not measured. Headless rendering may differ from a visible
  window's.
- `Performance.getMetrics` durations are cumulative and include the work of
  Playwright's input dispatch, which is the same for every variant.
- The fixed order of workloads within a page lets earlier workloads warm the
  code for later ones. The order is the same for every variant.
- Variant implementations are written for this lab; a different implementation
  of the same shape could perform differently.

## Decision and follow-ups

Outcome of the decision rule:

| Candidate | Primary metric | Constraints and limits | Outcome |
| --- | --- | --- | --- |
| 1. Class with helpers | Matches the control | All met | Adopt |
| 2. Setup with closures | Matches the control | Memory per instance +50% to +82%; cold start +24% to +29% | Reject |
| 3. Setup with shared handlers | Matches the control | Cold start +25% to +28%; everything else met | Reject |

Timing does not separate the shapes: interaction cost, script duration,
responsiveness, rendering and lifecycle steps are all within noise of the
hand-written control. They are separated by memory and one-time cost. Closures
per instance cost about 270 bytes per element. A generated class costs about
0.3 ms per page for one definition.

**Decision (2026-10-24):** elements are native classes written by their
authors, with narrow helpers for early-input capture and replay and for
registration. The owner decided before the confirmation run and stage 2. Both
were then run the same day:

- The confirmation reproduced variant 1's match with the control on the
  primary metric, the constraints, cold start and memory; cells at large sizes
  and the lifecycle steps were inconclusive under heavy machine load.
- Stage 2, under heavy load, showed variant 3's extra cold start does not
  scale with the number of definitions; it did not firmly establish a fixed
  per-page cost. Variant 3 meets the limit with 20 definitions and misses it
  narrowly with 5. Even where it passes, the rule's tie-breaker, memory per
  instance, favors variant 1: 310 against 336 bytes per added element.

The retained-memory constraint was not formally read; no variant retained
memory that grows with the elements created.

**Scope of the conclusion.** Native classes with narrow helpers match the
hand-written control on the confirmed interaction workloads and use less
measured memory than either setup shape. The lab does not show that classes
are faster in general: speed differences between the shapes were within noise,
and confirmation at 100 and 1,000 elements was inconclusive. Further authoring
labs are closed for now.

**Constructor application of inputs without an attribute** was approved on
2026-10-24 on correctness evidence: browser tests of early application,
constructor safety, mixed inputs, no capture, and parsed and cloned elements.
It applies only under the contract's conditions: the input has no attribute,
and its setter writes only the element's own nodes and fields.

Two adjustments were made while specifying the shape:

- A setter removes its input from the pending map, then compares with the
  rendered value. Variant 1 in the lab forced a write whenever it removed a
  pending entry; that write is redundant when the new value equals the
  rendered one.
- The stylesheet helper measured in variant 1 is not part of the shape;
  adopting a sheet is one assignment.

Follow-ups:

- [Element lifecycle](../specs/contracts/element-lifecycle.md) specifies the
  helpers, the pending-input rules, constructor application of inputs without
  an attribute, and this decision.
- The [elements guide](../specs/framework/elements.md) shows the authoring
  shape.
- A later stage confirms the shape through the real store and bindings once
  they exist.
