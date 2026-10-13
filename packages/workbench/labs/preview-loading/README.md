# Preview loading lab

Methodology: [Workbench labs](../../specs/labs.md). Owning contracts:
[performance](../../specs/preview-pipeline/performance.md) for the requirements and targets,
[TypeScript previews](../../specs/previews.md) for preview behavior.

Status: run set 1 reported on 2026-10-11; run set 2, on a separate day, is
needed before a decision. Plan agreed by the owner on 2026-10-11, before any
run. At agreement, the compiler comparison moved to its own
[compiler speed lab](../compiler-speed/README.md) and the host was corrected
to the machine available for the run.

Amendment, 2026-10-11, before any run: the owner added hot module replacement
with React Fast Refresh as a requirement
([performance](../../specs/preview-pipeline/performance.md#edits-apply-in-place)). The
question, variants 4 and 5, sequences S9 and S10, the HMR correctness checks
and the decision rule's HMR clause were added for it.

Amendment, 2026-10-11, after harness smoke runs and before any run set:

- S2 is measured in two cells. The primary cell (browser-warm) pauses one
  second before each selection, as someone looks at a page before choosing
  another; a back-to-back cell selects as soon as the previous page is
  visible. The plan did not state pacing, and work a variant does after a
  selection (variant 2's spare) can only help when there is time between
  selections. The walks use different previews, so each sample is still a
  first visit.
- Variants run from snapshots of this package with overlay files from
  `variants/<id>/overlay`, applied after the overlay of the variant named in
  `variants/<id>/variant.json` `extends`. Product code is not changed.
- S10's non-component module edit changes a feature's exported title, which
  the app shell shows on every page.
- S9 clicks the counter three times with time to render between clicks (a
  burst in one task is one React update), records the state before the edit,
  and counts state as kept only when it was set before and is unchanged
  after. Its reset between samples waits for the reloaded frame itself.
- Variants 1 to 3 keep project builds in the preview disk cache, as the
  shipping compiler keeps per-preview builds, and leave failing entries out
  of the shared build until a file they read changes. Variant 4 keeps its
  pre-bundled dependencies in the same cache, keyed by the project's manifest
  and lockfile, and serves module requests concurrently. Smoke runs showed
  that without these, every edit and every start repeated work the shipping
  path caches, which would have compared an unfinished variant.
- The mixed fixture and S7 are not implemented and are reported as
  unmeasured. Diagnostic controls D1 to D3 run as variants `d1`, `d2` and
  `d3`.

## Plan

### Question

How should a project's TypeScript previews compile, load and reuse frames so
that selecting another preview in the same project becomes visible quickly,
and edits apply in place with hot module replacement and React Fast Refresh,
without losing isolation, freshness or error containment? The answer decides
whether Workbench changes its compile output, its serving model, its frame
lifecycle, or a combination. Which compiler is fastest is the
[compiler speed lab](../compiler-speed/README.md)'s question; this lab uses
esbuild except where a variant needs a capability esbuild lacks.

### Hypothesis

Most of the wait on a large project is the browser loading and evaluating a
complete bundle for each preview. Each preview is a monolithic bundle that
repeats the same dependencies and application code, and a newly selected
preview usually gets a new frame document. A project-wide build with shared,
content-hashed chunks, plus a spare frame that has already loaded those
chunks, should cut selection to visible by most of the module-load time.

Evidence behind it, from an exploratory investigation on 2026-10-11 against a
selected local React project (two page previews sharing one environment;
macOS, Chrome, Workbench 0.13.1). This was gathered before the plan and is not
evidence for the decision:

- Server-side work for cached descriptor requests was 12–26 ms; one was
  217 ms.
- In one fresh selection, 1.85 s passed between the descriptor response and
  the preview runtime starting, for a 5.1 MB module. Mount then took about
  120 ms, followed by about 0.5 s in the font phase (font status already
  `loaded`) and about 0.65 s in the image phase (two pending images).
- Returning to a retained frame took 82–133 ms.
- Rebuilding both previews as one code-split esbuild build gave a 4.79 MB
  shared chunk plus 219 KB and 352 KB of preview-specific code.
- Cold monolithic compiles took 2.7–11 s per preview; the split build of both
  took about 1.8 s. A no-change esbuild context rebuild took about 1.6 s,
  attributed to the project's config plugins reloading files on every build.

What the font and image phases contain is not established. They may be React
development-mode work and data effects after mount rather than fonts or
images.

### Variants

| # | Variant | Supported subset and missing contracts |
| --- | --- | --- |
| 0 | **Control.** Shipping behavior: one monolithic esbuild bundle per preview, one compile worker per project, frames retained per session | Full |
| 1 | **Shared chunks.** One esbuild build per project and adapter with every discovered preview as an entry, `splitting` on, content-hashed chunks served as immutable; frame lifecycle unchanged | React and React Native Web first; Vue and HTML if the split build accepts them; Astro server renders stay separate |
| 2 | **Shared chunks with a warm spare.** Variant 1, plus one spare frame per project and adapter that loads and evaluates the shared chunks before it is needed; each selected preview still gets its own document | As variant 1 |
| 3 | **Single document per project.** Variant 1, with one long-lived frame per project and adapter that unmounts one preview and imports the next, as Storybook does | As variant 1. Eligible only if it passes the isolation checks |
| 4 | **Unbundled dev server with HMR.** Dependencies pre-bundled once per project with esbuild; project source transformed per module and served as native ES modules, with an HMR runtime (`import.meta.hot`), server push over a WebSocket and React Fast Refresh. esbuild has no Fast Refresh transform, so project modules use Oxc's transformer with its refresh option, pinned in the lab | React first. Config plugins that need a whole-graph build are listed as unsupported |
| 5 | **Bundled with built-in HMR.** Shared chunks as in variant 2, built by a bundler with its own HMR runtime and React Fast Refresh in development mode. The bundler is chosen by a rule fixed now: the fastest in the compiler speed lab's B3 and B4 among candidates whose HMR and Fast Refresh work programmatically. If the compiler lab has not reported when this lab runs, it uses Rspack, the candidate whose HMR and React refresh plugin are documented for programmatic use | As variant 1, limited to what the chosen bundler's plugins can express |

Variants 0 to 3 build esbuild bundles, which have no module runtime that can
replace one module in place. They show what loading costs without HMR; their
edits reload the frame, as the control does.

Diagnostic controls, not candidates:

- **D1:** variant 0 with readiness marked immediately after mount, to
  measure what the font and image phases cost.
- **D2:** variant 0 built with `process.env.NODE_ENV` set to `production`, to
  measure React development-mode cost.
- **D3:** variant 0 without linked source maps.

### Fixtures

An Acme generator, seeded, at three sizes. Every size uses React, one project
environment that wraps previews in providers, and request mocks.

| Size | Shape |
| --- | --- |
| Small | 5 component previews, about 300 KB of dependencies, no fonts or images. The shipping behavior already handles this well |
| Large | 40 page previews sharing about 5 MB of dependencies and application code (React DOM, an Apollo-style GraphQL client, lodash-es, a router, a table library, locale data split into dynamic imports), two web fonts and two images per page |
| Mixed | The large fixture plus 5 Vue previews and 5 React Native Web previews, for per-adapter graphs |

Every fixture includes:

- a module-level store and a global listener sentinel in its environment, to
  detect state leaking between previews;
- a config plugin that reads files in `onLoad`, to reproduce plugin-bound
  rebuild cost;
- one preview that fails to compile and one that throws during mount.

An explicitly selected local project may be run as an additional, read-only
input under the rules in the methodology. Its results are reported by shape
only.

### Sequences

| ID | Action |
| --- | --- |
| S1 | Open the first preview after the server starts |
| S2 | Select a different preview in the same project and adapter |
| S3 | Return to a retained preview |
| S4 | Force a reload of the current preview |
| S5 | Edit the current preview's own source |
| S6 | Edit a module shared by every preview |
| S7 | Select a preview with a different adapter (mixed fixture) |
| S8 | Select the broken previews, then a working one |
| S9 | Change a stateful React component's state through the page (a counter and a typed field), then edit that component's render output |
| S10 | Edit a non-component module that the current page imports, then the preview definition, then the project environment |

S9 measures edit to visible and checks that the state survives. S10 checks
which edits apply in place and which fall back to a reload, against
[the requirement](../../specs/preview-pipeline/performance.md#edits-apply-in-place).

S2 runs as a walk through ten previews not yet opened, so each sample is a
first visit to that preview.

### Boundaries and cache states

Timers follow the methodology's boundaries. The primary boundary is selection
to visible. Every sequence also reports compile, module load, mount and
readiness.

| Sequence | Cache states |
| --- | --- |
| S1 | Cold, disk-warm |
| S2, S7 | Browser-warm |
| S3 | Retained |
| S4 | Browser-warm |
| S5, S6, S9, S10 | Browser-warm, with the compiler holding the previous build |
| S8 | Browser-warm |

### Correctness checks

All checks in the methodology, on every run, for every variant and diagnostic
control. Render parity compares every state of ten previews per fixture at the
laptop and mobile sizes. A diagnostic control may fail readiness parity by
design; it is reported, not adopted.

HMR checks, for variants 4 and 5 (variants 0 to 3 report "reload" for each):

- a component edit in S9 updates the page without a document reload, keeps
  the counter value and the typed text, and shows the edit;
- each S10 edit either applies in place or reloads, as the requirement
  says, and the selected state, size and input overrides survive;
- a syntax error saved during S9 shows in the frame, and fixing it recovers
  without a manual reload;
- after any sequence of hot updates, the page renders the same as a fresh
  load of the edited source.

### Metrics

- **Primary:** S2 selection to visible on the large fixture.
- **Secondary:** S9 edit to visible; S1, S4, S5, S6 and S7 selection or edit
  to visible; compile
  time per sequence; bytes transferred and evaluated per selection; worker CPU
  and peak RSS; JS heap per retained frame; preview disk-cache size;
  long tasks during readiness.

### Runs

A warmup of three samples per cell, then at least ten samples per cell.
Variants are alternated in a seeded order within each run set. Two run sets on
separate days before a decision.

### Decision rule

The targets are the proposed ones in [performance](../../specs/preview-pipeline/performance.md#measured-interactions):
for S2 on the large fixture, p50 at or below 300 ms and p95 at or below
600 ms, and no sequence on the small fixture slower than the control by more
than 10%. They are proposed, not agreed. If the owner changes them before the
first run, this plan follows; after it, a change is a dated amendment.

- A variant that fails any correctness check is rejected.
- HMR with React Fast Refresh is required, so only variants that pass the HMR
  checks can be adopted. Among them, adopt the simplest one whose S2 p50 is
  within 10% of the fastest. Variant order in the table runs from least to
  most change.
- Variants 0 to 3 are not adoptable on their own. If one of them loads pages
  clearly faster than every HMR-capable variant, the report says so and names
  the loading technique a follow-up should combine with HMR.
- If a difference falls within the spread of the two run sets, the result is
  inconclusive for that comparison.
- If the diagnostic controls show that readiness, not module loading,
  dominates after the winning variant, a follow-up lab covers readiness.

### Host and tools

macOS 15 on Intel (Xeon W-2140B, 16 threads, 64 GB). Apple silicon and other
platforms are not measured. Headless Chrome through the standalone server,
using the package's DevTools driver in `scripts/chrome.cjs`, for run sets. The
VS Code webview with the bundled Electron runtime for the confirmation run.
esbuild is the version the package pins (0.28.2). Variant 4's Oxc
transformer, variant 5's bundler and its React refresh plugin, and the
`react-refresh` runtime are pinned in the lab. Fixture dependencies are
pinned by the [shared Acme fixture](../fixtures/README.md) and installed into
the run directory.

Budget and authorization: the owner authorized installing fixture
dependencies from the npm registry into run directories on 2026-10-11.
Nothing is installed into the package or any project, and no other external
service is contacted.

## Running the lab

```sh
node labs/preview-loading/run.mjs --variants 0,1,2,3,4,5 --source-revision <measured commit> --results labs/preview-loading/results/<date>-<label>
```

`--smoke` takes two samples per cell with no warmups, to check the harness.
`--fixtures`, `--sequences` and `--samples` restrict a run; `--no-renders`
skips render parity, `--debug` prints server warnings and worker errors, and
`--keep` keeps the run directory's snapshots, fixtures and caches.

The runner builds a snapshot of this package per variant in a run directory
under the system temporary folder, with the variant's overlay from
`variants/<id>/overlay` (after the overlay of the variant its `variant.json`
extends). `serve.cjs` starts a snapshot's server for a fixture with capture
stubbed out and its preview disk cache in a directory the runner chooses.
Headless Chrome, through the package's DevTools driver, drives the canvas by
its address and times each selection inside the canvas page, from the address
change to the second animation frame after `wb-frame-change`. Edit sequences
write fixture files and time until the active frame shows a marker. Variants
4 and 5 load lab-pinned packages from `tools/package.json`, installed into the
run directory.

| Variant | Overlay |
| --- | --- |
| 1 | `preview/compiler.cjs` gains an entry-only mode; `preview/project-build.cjs` builds every project-buildable preview of an adapter as one code-split esbuild build; `preview/worker.cjs` serves it with immutable chunk caching |
| 2 | Variant 1, plus `workbench/workbench.js` keeping a spare host frame and `workbench/preview-host.js` loading the project's shared chunks into it |
| 3 | Variant 1, plus `workbench/workbench.js` replacing the current preview in the same host document |
| 4 | Variant 1's compiler, plus `preview/esm-server.cjs` (Oxc transforms, pre-bundled dependencies, HMR over server-sent events, React Fast Refresh) and its `preview/worker.cjs` |
| 5 | Variant 1's compiler, plus `preview/rspack-dev.cjs` (Rspack watch build with HMR and React Refresh, per-preview module shims) and its `preview/worker.cjs` |
| d1 | `preview/browser.js` marks a preview ready right after mount, without the font and image waits |
| d2 | `preview/compiler.cjs` builds with `process.env.NODE_ENV` set to `production` |
| d3 | `preview/compiler.cjs` builds without linked source maps |

Not measured: the mixed fixture (Vue and React Native Web previews) and its
sequence S7 are not implemented by the fixture generator; their cells are
reported as unmeasured.

## Report

Run set 1 only. The plan requires a second run set on a separate day before
a decision; the result below is provisional.

Run set 1 ran from 14:26 to 15:57 UTC on 2026-10-11 at Workbench commit
`63a096f`, alone under the lab lock, with 3 warmups and 10 samples per cell,
on the host described above, in headless Chrome 154. Evidence:
[summary](results/2026-10-11-set-1/summary.md) and its `report.json`, which
records each result's load average and busiest other processes.

### Results, large fixture

p50 / p95 in milliseconds; S5, S6 and S9 are edit to visible.

| Sequence | 0 control | 1 shared chunks | 2 + warm spare | 3 single document | 4 unbundled HMR | 5 Rspack HMR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| S1 first preview, cold | 6817 / 8474 | 3961 / 4305 | 4133 / 4607 | 4144 / 4821 | 3668 / 3768 | 3685 / 4010 |
| S1 first preview, disk-warm | 661 / 857 | 598 / 638 | 612 / 661 | 600 / 701 | 835 / 912 | 1325 / 1454 |
| **S2 new preview, paced** | **1425 / 1582** | **196 / 209** | **132 / 154** | **115 / 130** | **481 / 582** | **343 / 389** |
| S2 new preview, back to back | 1373 / 1559 | 192 / 210 | 154 / 182 | 113 / 148 | 446 / 569 | 325 / 354 |
| S3 retained preview | 61 / 64 | 77 / 85 | 80 / 193 | 106 / 111 | 49 / 50 | 47 / 51 |
| S4 reload | 143 / 163 | 144 / 180 | 113 / 138 | 104 / 110 | 432 / 457 | 294 / 299 |
| S5 page edit | 2255 / 2283 | 783 / 809 (7 lost) | — (12 lost) | 1151 / 1188 | 48 / 65 | 112 / 159 |
| S6 shared edit | 4511 / 6619 | 1669 / 1725 | 1736 / 1870 (5 lost) | 1348 / 1458 | 65 / 73 | 124 / 134 |
| S9 component edit | 1973 / 2057 | 793 / 1545 | 1433 / 1466 | 1414 / 1424 | 58 / 68 | 119 / 158 |

On the small fixture, S2 took 251 ms for the control, 57–79 ms for variants
1 to 3, 94 ms for variant 4 and 114 ms for variant 5 (one sample each after
warmups; the small fixture has only four previews to walk). Its page and
shared edits took 598 and 828 ms for the control, 438–567 ms for variants
1 to 3, 47–49 ms for variant 4 and 620–742 ms for variant 5.

### HMR

| Check | 0 | 1 | 2 | 3 | 4 | 5 |
| --- | --- | --- | --- | --- | --- | --- |
| S9 state kept (counter and text) | 0 of 10 | 0 of 10 | 0 of 10 | 0 of 10 | **10 of 10** | **10 of 10** |
| S10 module edit | reload, 4761 ms | reload, 1503 ms | reload, 1443 ms | reload, 1919 ms | in place, 196 ms | in place, 166 ms |
| S10 environment edit | reload, 4321 ms | lost | lost | lost | reload, 252 ms | document reload, 5150 ms |

### Correctness

- **Render parity:** every variant and diagnostic control matched the
  control on every captured preview, state and size (20 of 20 small, 40 of 40
  large).
- **Isolation:** variant 3 leaked module state between previews in 16 of 20
  small and 36 of 40 large renders (the environment module saw more than one
  preview). No other variant leaked.
- **Error containment:** every variant showed both broken previews' errors
  and then loaded a working preview. Variant 5's compile error took 35 s to
  show, because of the lab's own code. Rspack builds a preview with a missing
  import and reports the error. The built entry throws when it runs, so it
  never registers, and the lab's preview module polled for the registration
  until its 30 s deadline. Variant 5 should show build errors from Rspack's
  stats at once. This is a variant defect, not a cost of Rspack's build.
  Variant 5's cold first open, which builds every preview, took 3.7 s, the
  same as variant 4's.
- **Clean console:** variant 5 logged 32 exceptions on the large fixture:
  hidden retained preview frames received hot updates their Rspack runtime
  could not apply. Every other variant logged none.
- **Freshness:** edits were lost in variants 1 and 2 (S5, S6 and S10
  environment) and in diagnostic control D1, but not in the control. See the
  discovery below; the loss comes from the shipping compiler's cache, which
  variants 1 to 3 inherit.

### Discovery: the shipping compiler caches stale output

When a source file changes while Workbench is compiling a preview, the
compiler records the file's new timestamp but the build's output reflects the
old content, so the stale output is served as current until the next edit. A
probe against the shipping `Compiler` (2026-10-11, large fixture) wrote edit
A, started a compile, wrote edit B 150 ms later, and got A's output from both
that compile and the next one while the file held B. Variants 1 to 3 copy the
same timestamp-after-build pattern in their project build. The control
escaped in this run only because its slower readiness spaced the harness's
edits further apart; someone who saves twice within about a second can hit
it today. Variants 4 and 5 use their own file watchers and were not affected.
This is a product defect independent of the lab's decision.

### Diagnostic controls

Against the control's paced S2 of 1425 ms on the large fixture: removing the
font and image waits (D1) gave 1316 ms, a production React build (D2) 1280 ms,
and no linked source maps (D3) 1181 ms. On this fixture readiness waits cost
about 0.1 s and development React and source maps about 0.15–0.25 s each.
The motivating project's longer font and image phases are not reproduced by
the fixture's local fonts and two small SVGs.

### Analysis

- Most of the control's time to show a new preview is loading its own
  complete bundle. Sharing chunks (variant 1) cut paced S2 from 1425 ms to
  196 ms; a spare frame that has already loaded them (variant 2) cut it to
  132 ms. Both meet the proposed S2 target. Their cold first open is also
  faster than the control's, and their disk-warm first open matches it.
- Variant 3 is fastest but fails isolation, as expected.
- Variant 4 delivers HMR with React Fast Refresh at about 50–65 ms per edit
  and keeps state, but loads new previews in 481 ms: each preview fetches its
  project modules one by one. It misses the proposed S2 p50 target, and its
  reload is slower than the control's.
- Variant 5 also keeps state, at about 110–160 ms per edit, and loads new
  previews in 343 ms, but logs exceptions in retained frames and its
  small-fixture edits are not faster than the control's.
- Threats to validity: one Intel host; one run set; a synthetic fixture whose
  fonts and images load locally; headless Chrome rather than the VS Code
  webview; the mixed fixture and S7 are unmeasured; small-fixture S2 has one
  sample per cell.

### Provisional result against the decision rule

- Rejected: variant 3 (isolation), variant 5 (clean console).
- Variants 0 to 2 have no HMR and are not adoptable on their own. Variants 1
  and 2 load previews clearly faster than every HMR-capable variant (132–196
  ms against 343–481 ms), so, as the rule asks, the report names the loading
  technique a follow-up should combine with HMR: a project-wide shared build
  with content-hashed chunks, loaded ahead into a spare frame.
- Variant 4 is the only variant that passes the HMR checks without a
  correctness failure, but it misses the proposed S2 target and a
  small-fixture sequence (S4) by more than 10%.
- Result: inconclusive pending run set 2. The evidence points to a follow-up
  that combines variant 2's shared, pre-loaded chunks with variant 4's HMR.
  The stale-cache defect should be fixed in the product regardless.
