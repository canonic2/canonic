# Workbench labs

## Purpose and ownership

A lab answers one engineering question with a reproducible comparison before
Workbench commits to an implementation. It runs working variants of the
proposed change beside the shipping behavior, checks that each one renders the
same pages correctly, and measures what the person at the canvas actually waits
for. Its output is a decision backed by evidence, not code to ship.

Run a lab when a choice depends on behavior that reasoning, existing tests or
documentation cannot settle: how long a page takes to appear, what a browser
caches or throttles, how a compiler or bundling strategy behaves on a large
project graph, or what a runtime costs in memory. Do not run a lab to confirm a
decision already made, or in place of behavioral tests for product code.

This spec owns experiment design, execution, evidence and confirmation. Feature
specs own product behavior and any agreed budgets: [TypeScript previews](previews.md)
for previews and the Node evaluator, the [preview pipeline](preview-pipeline/README.md)
for compilation, delivery, hot updates, readiness and performance targets,
[core](core.md) for the canvas, frames and selection, [Storybook](storybook.md)
for story reuse and [capture](capture.md) for screenshots. Lab findings inform those contracts;
they do not change them silently.

## Locations and lifecycle

Labs live under `packages/workbench/labs/<slug>/`, with a short kebab-case slug
naming the question:

```text
labs/<slug>/
  README.md          plan, then report and decision
  run.*              runner(s) and variant implementations
  fixtures/          deterministic fixture generators, never generated output
  results/<date>/    reviewed evidence from one run set
```

A fixture generator used by more than one lab lives in `labs/fixtures/`, with
its own README describing what it generates and the pinned dependencies.

- Labs belong to the public repository with the rest of the package, so
  everything committed under `labs/` follows the fixture and evidence rules
  below. `extension/.vscodeignore` excludes `labs/` from the VSIX. Product code, tests,
  packaging and scripts never import, run or name anything in `labs/`.
- Runners may load the shipping modules read-only, such as
  `server/node/preview/compiler.cjs` and `server/node/server.js`, and the
  `workbench-server` binary. Variant logic
  lives in the lab. A lab changes no product code; the winning variant is
  re-implemented in the product with its own tests.
- Generated projects, installed fixture dependencies, build output, compiler
  caches, browser profiles and routine reports live in an owned temporary
  directory per run. A runner points every cache it can reach at that
  directory, including the preview disk cache under the system temporary
  folder, so runs neither read nor pollute the user's caches.
- One-off probes that are not part of a planned lab stay in temporary folders
  and are never committed; their findings go in the owning spec.
- When the decision is recorded and the winning change has shipped, delete the
  runner and variant code. Keep `README.md` and the reviewed `results/` as the
  record, with the source revision that produced them. The feature spec
  describes the chosen behavior without depending on the lab.

Every runner closes the servers, compilers, browsers and child processes
it starts, on success, failure and cancellation, and reports cleanup failures.

## Fixtures and selected projects

- Committed fixtures are deterministic generators for neutral Acme projects
  that use example.com. A generator takes a seed and a size, and emits a
  project with `workbench.yaml`, a `workbench.config.ts`, previews, an
  environment and pinned dependencies. The generated shape is recorded:
  preview count, adapters, source modules and bytes, dependency packages,
  fonts, images and request mocks.
- Each lab includes the motivating shape and at least one shape the shipping
  behavior already handles well, so a variant cannot win by regressing small
  projects.
- A runner may also take an explicitly selected local project path at run
  time. The lab treats that project as read-only. All output, caches and
  browser profiles go to the run's temporary directory, and its files, Git
  state and installed dependencies are never changed. Committed evidence
  describes a selected project only by its measured shape. It never contains
  the project's name, paths, source, screenshots or request data. Results that
  cannot be reduced that way stay out of `results/`.
- Installing fixture dependencies is the only expected network use. Install
  them into the run directory from a lab lockfile; never add lab dependencies
  to the package's `package.json` or lockfile.

## The plan

Write the plan in the lab's `README.md` and agree it with the owner before the
first run. A plan states, before any results exist:

| Section | Content |
| --- | --- |
| Question | The one question the lab answers, and the decision it informs |
| Hypothesis | The expected outcome and why, with the evidence that suggested it |
| Variants | Each approach under test. Variant 0 is always the control: shipping behavior, unchanged. Name each variant's supported subset and missing contracts |
| Diagnostic controls | Variants that isolate a cost, such as disabling a readiness wait, and are not candidates for adoption |
| Fixtures | Generators, seeds and sizes; the same inputs for every variant |
| Sequences | The ordered user actions measured, such as opening a first page or switching to another page in the same project |
| Boundaries | Where each timer starts and stops, and which process owns it |
| Cache states | The compiler, disk-cache and browser cache state for each cell |
| Correctness checks | What every variant must still do, checked on every run |
| Metrics | Primary and secondary measurements, and how they are summarized |
| Runs | Samples per cell, warmups, run order and repetitions |
| Decision rule | What makes a variant win, lose or leave the question inconclusive. Proposed targets are labelled as proposed |
| Host and tools | Browser, runtime, compiler versions, and the confirmation surface |

Do not choose the winning metric, drop an unfavorable sequence or change a
threshold after seeing results. A change to the plan after a run has started
is recorded as a dated amendment with its reason.

## Compare real implementations

Each variant runs an implementation of the proposed work through the same HTTP
routes and canvas the product uses, with its build, serving and frame
lifecycle costs visible. Research, bundle-size estimates and component timings
can select variants to try; they cannot replace a measured sequence.

Keep fixtures, sequences, viewport sizes, states and checks identical across
variants. A variant that implements only part of a contract, such as React
previews but not Astro, reports results for that subset, and the missing
contracts stay listed. A compiler variant must accept the fixture's
`workbench.config.ts` aliases, defines and plugins, or the lab records which
ones it could not run.

## Boundaries

Name every timed boundary from this list, or define a new one in the plan:

| Boundary | Starts | Stops |
| --- | --- | --- |
| Compile | The server receives a descriptor or page request | The response is written |
| Module load | The preview frame requests its module | The preview runtime reports `started` |
| Mount | Runtime `started` | `mount.completed` |
| Readiness | `mount.completed` | `__workbenchReady` |
| Selection to visible | The canvas receives the page selection | The new frame is promoted and painted |
| Edit to visible | A fixture source file is written | The canvas shows output that includes the edit |

Selection to visible is the user-facing measurement; the other boundaries
explain it. Use the existing `preview.request.*` and `preview.browser.phase`
diagnostics, browser resource timing and canvas frame-change events before
adding instrumentation. Record instrumentation added by the lab, and check that
it does not change the result.

State each cell's cache state:

- **Cold:** a new compiler process, an empty preview disk cache and a new
  browser profile.
- **Disk-warm:** a new compiler process and a new browser profile, with the preview disk
  cache populated by an earlier run of the same variant.
- **Browser-warm:** the same browser profile, with HTTP and code caches
  populated, and a new frame.
- **Retained:** a frame the canvas still holds for the page.

Operating-system file caches are uncontrolled unless the method establishes
otherwise; say so rather than calling a cell filesystem-cold.

## Correctness checks

A faster variant that renders something different, leaks state or hides an
error is rejected. Every lab that changes how previews compile, load or mount
checks, on every run:

- **Render parity:** each page and state matches the control's screenshot at
  the same size, with request mocks, time and randomness fixed by the fixture.
- **Isolation:** module-level state, global listeners, styles, request mocks
  and environment providers from one preview do not appear in another. The
  fixture includes sentinels that detect each leak.
- **Freshness:** an edit to a preview's own source and to a shared module both
  reach the canvas, and an unchanged page is not served stale output.
- **Error containment:** a preview that fails to compile or throws during mount
  shows its error without breaking other previews in the same project.
- **Readiness and capture:** `__workbenchReady` still waits for the same fonts
  and images, and screenshots match the control.
- **Clean console:** no errors or warnings that the control does not produce.

## Measurement

- Use a monotonic clock: `performance.now()` in browser documents and
  `process.hrtime.bigint()` in Node. Convert browser timestamps through a
  shared origin rather than comparing wall clocks across processes.
- Measure in the bundled Electron runtime or headless Chrome through the
  standalone server. Hidden-frame throttling, GPU and code caching differ
  between hosts. A result that the decision relies on is confirmed in the
  VS Code webview before adoption.
- Record bytes transferred and decoded per frame, scripts evaluated, long
  tasks during readiness, compiler-process CPU and peak RSS, JS heap per
  retained frame and preview disk-cache size where the plan names them.
- Take at least ten samples per cell for latencies, after a declared warmup,
  with variants and sequences alternated in a seeded order. Report sample
  counts, p50, p95 and maximum by nearest rank, and keep the raw samples. Ten
  samples do not establish stable tails; label p95 as exploratory unless the
  plan collects more.
- Keep first-use costs, such as the first compile after a compiler starts or
  the first frame of a project, as separate cells instead of folding them
  into warm averages.
- Avoid builds, test suites or other heavy work on the host while measuring.
  If overlap occurs, keep the result and qualify or rerun it rather than
  silently choosing a quieter sample.

Record the host and conditions with every run: macOS version, architecture,
CPU and memory, power source, Node, Electron, Chrome and esbuild versions,
other compiler versions used by variants, and background load. Record the
Workbench commit that was measured, plus hashes of any uncommitted measured
files, since a commit ID alone does not identify uncommitted code.

## Runner reliability

Measurement code must satisfy the same reliability and ownership standards as
product code. Runner defects must not appear as product timings or verdicts.

### Waits and stop conditions

- Stop on the requested page's content and generation, not any frame change,
  readiness message or outgoing error. A warm document can navigate without
  swapping frames.
- Distinguish edited and original values exactly; substring matches cannot
  establish which revision is visible.
- Compare both copies only after they reach the same settled data state.
- An error ends a wait when the sequence expects an error. Readiness waits
  must detect failure explicitly rather than wait on an error page forever.
- Put runner markers in owned content. Page markup can replace body/html
  attributes, so those attributes are not stable measurement markers.
- Use events or tight polling with explicit deadlines. Report a timeout with
  the observed state, never as a successful latency sample.
- Wait for a product state through a request the product answers, not for
  a diagnostic it may never emit: when work starts lazily, the event
  announcing it never comes.
- Independently observe fonts, images and visible content when stopping a
  timer. The product's readiness flag alone is not the correctness oracle.

### Comparisons and verdicts

- Exclude only differences declared by the variant design, such as generated
  asset URLs or stylesheet delivery form. Record every exclusion and reason.
- Compare project files by content; normalized timestamps cannot establish
  whether the project stayed unchanged.
- Keep correctness verdicts separate from build/protocol fields. A field from
  variant output must never overwrite a failed runner assertion.
- Read structured reports for verdicts. Live logs help diagnose a stuck run;
  filtering a log cannot establish that every check passed.

### Caches, files and processes

- Give the run its own cache directories. Measure compiler-cache size
  separately from extracted runtimes, fixture dependencies and browser profiles.
- Record saved-index, compiler-cache, runtime-preparation and browser-cache
  states independently. A warm compiler does not imply a warm page index.
- Restore edited fixture files in `finally`, and wait for the original output
  before beginning the next sequence.
- A declared setup retry is bounded and recorded. Do not retry failing
  correctness or timing samples until a favorable result appears.
- Close every server, compiler, browser and child process on completion, failure
  and cancellation; report cleanup failures.

### Before and during a run set

- Smoke every group with the control and one candidate on every fixture.
  Classify failures as runner defects or findings before starting the set.
  Declare any expected control failures in the plan.
- A finding during the set does not change its checks or thresholds. Finish
  and record the finding; amend the method before a subsequent run set.

## Reports and decisions

Each run set writes a machine-readable `report.json` and a short summary into a
new `results/<date>[-<label>]/` folder. A runner refuses to overwrite an existing
folder; a rerun adds a new one. The report identifies the plan, variants,
fixtures and seeds, commands, host conditions, raw samples and aggregates,
correctness outcomes, failures, partial runs and cleanup failures.

The README's report section, below the plan, adds:

- one results table per metric, with p50, p95 and spread per variant and cell;
- the correctness outcome for every variant and check;
- an analysis of how far the evidence supports the hypothesis, and the
  threats to validity, such as fixture shape, host load or headless-only
  measurement;
- the decision against the decision rule (adopt, reject or inconclusive),
  and the specs to update.

A decision follows these steps:

1. Verify fixture identity, correctness outcomes and sample counts, and
   recompute aggregates from the raw samples.
2. Repeat control and the leading variant under comparable conditions. If the
   difference falls within run-to-run variation, record the result as
   inconclusive.
3. After the winning change is built into the product, rerun the lab's
   sequences through the shipping server and the VS Code webview. Prototype
   timings do not establish the integrated result.
4. Record the decision and its scope in the owning feature spec, with the
   measured conditions. Keep exact values with their evidence instead of
   copying tables across specs. Do not relax a correctness check to fit the
   fastest variant.

## Lab records and active follow-up

Lab plans and reports retain experiment history. Feature specs own the current
product contracts; a report or an unadopted variant is not a shipping interface.

| Record | Current relevance and remaining work |
| --- | --- |
| [Preview loading](../labs/preview-loading/README.md) | Shared-build and runtime design evidence; current navigation requirements belong to [performance](preview-pipeline/performance.md) |
| [Rspack pipeline](../labs/rspack-pipeline/README.md) | Evidence for the implemented compiler, warm document and hot updates; shipping-server/webview confirmation remains open |
| [First page](../labs/first-page/README.md) | Startup-scaling comparisons; no replacement start is adopted by the active compiler contract |
| [Instant first page](../labs/instant-first-page/README.md) | Evidence for the implemented [page index](preview-pipeline/page-index.md), and the [compiler-host comparison](../labs/instant-first-page/README.md#stage-2-report) through the shipping canvas: reported, no variant meets the agreed targets, decision pending |
| [Compiler speed](../labs/compiler-speed/README.md) | Compiler-selection evidence; the active development/portable compiler is Rspack |
