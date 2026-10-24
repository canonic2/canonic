# Preview loading

## Question

How should a project's TypeScript previews compile, load and reuse frames so
that selecting another preview in the same project becomes visible quickly,
and edits apply in place with HMR and React Fast Refresh, without losing
isolation, freshness or error containment? The answer decides whether
Workbench changes its compile output, its serving model, its frame lifecycle,
or a combination. Compiler speed is the [compiler speed](compiler-speed.md)
question.

## Status

Reported, provisional: run set 1 on 2026-10-11. The plan required a second
run set on a separate day before a decision; it did not run. The evidence
pointed to combining shared, pre-loaded chunks with HMR, which the
[Rspack pipeline](rspack-pipeline.md) then tested. The stale-output defect
found here was a product defect independent of the decision.

## Hypothesis

Most of the wait on a large project is the browser loading and evaluating a
complete bundle for each preview: each preview is a monolithic bundle that
repeats the same dependencies, and a newly selected preview usually gets a
new frame document. A project-wide build with shared, content-hashed chunks,
plus a spare frame that has already loaded them, should cut selection to
visible by most of the module-load time.

Exploratory evidence before the plan (a selected local React project, two
page previews): cached descriptor requests took 12–26 ms server-side; one
fresh selection spent 1.85 s between the descriptor response and the runtime
starting, for a 5.1 MB module, then about 120 ms mounting and about 1.15 s in
font and image phases; retained returns took 82–133 ms; cold monolithic
compiles took 2.7–11 s per preview, a split build of both about 1.8 s.

## Method

**Variants.**

| # | Variant | Supported subset |
| --- | --- | --- |
| 0 | Control: one monolithic esbuild bundle per preview, one compile worker per project, frames retained per session | Full |
| 1 | Shared chunks: one esbuild build per project and adapter, every preview an entry, `splitting` on, content-hashed immutable chunks | React and React Native Web first |
| 2 | Variant 1 plus a warm spare frame per project and adapter that has loaded the shared chunks; each selection still gets its own document | As 1 |
| 3 | Variant 1 with one long-lived document per project and adapter that unmounts one preview and imports the next | As 1; eligible only if isolation passes |
| 4 | Unbundled dev server: dependencies pre-bundled once with esbuild, project modules transformed by Oxc with its refresh option and served as native ES modules, HMR over server push, React Fast Refresh | React |
| 5 | Bundled with built-in HMR: shared chunks built by Rspack in development mode with its HMR runtime and React Refresh plugin | As 1 |

Diagnostic controls: D1 readiness marked right after mount (no font and image
waits); D2 a production React build; D3 no linked source maps.

**Fixtures.** The [shared Acme fixture](../fixtures/labs/README.md):
`small` (5 component previews) and `large` (40 page previews sharing about
5 MB of code, two web fonts and two images per page), each with module-level
store and listener sentinels, a file-reading config plugin and two broken
previews. The planned mixed-adapter fixture was not implemented.

**Sequences.** S1 first preview after start; S2 a new preview in the same
project (a walk through ten unopened previews, paced with a one-second pause,
plus a back-to-back cell); S3 return to a retained preview; S4 reload; S5 own
source edit; S6 shared module edit; S8 broken previews then a working one;
S9 change React state then edit the component; S10 edits to a non-component
module, the definition and the environment. S7 (adapter switch) was
unmeasured.

**Boundaries and cache states.** Primary: selection to visible, timed inside
the canvas from the address change to the second animation frame after
`wb-frame-change`. Edits time from the file write to the active frame showing
a marker. S1 cold and disk-warm; S2, S4 browser-warm; S3 retained; edits
browser-warm with the compiler holding the previous build.

**Correctness checks.** Render parity of every state of ten previews at laptop
and mobile sizes; isolation sentinels; freshness; error containment;
readiness and capture; clean console. For HMR variants: a component edit keeps
counter and typed text without a reload; each S10 edit applies in place or
reloads as the requirement says; a syntax error recovers without a manual
reload; after hot updates the page matches a fresh load.

**Runs.** Three warmups and ten samples per cell, variants alternated in a
seeded order; two run sets on separate days were planned.

**Decision rule.** Proposed targets: S2 on the large fixture p50 ≤ 300 ms,
p95 ≤ 600 ms; no small-fixture sequence more than 10% slower than control. A
variant failing a correctness check is rejected. HMR with Fast Refresh is
required, so only variants passing the HMR checks are adoptable; among them
the simplest within 10% of the fastest S2. If a non-HMR variant loads clearly
faster, the report names the technique to combine with HMR.

**Amendments, 2026-10-11.** Before any run, the owner added HMR with React
Fast Refresh as a requirement (variants 4–5, S9–S10, HMR checks). After
smoke runs: S2 gained the paced and back-to-back cells; variants run from
package snapshots with overlays; variants 1–3 keep project builds in the
preview disk cache and variant 4 caches its pre-bundled dependencies, so they
do not repeat work the shipping path caches.

## Conditions

Run set 1 on 2026-10-11 at Workbench `63a096f`, macOS 15 on Intel (16 threads,
64 GB), headless Chrome 154 through the standalone server, one lab at a time,
3 warmups and 10 samples per cell. esbuild 0.28.2; Oxc, Rspack and the React
refresh runtime pinned in the lab. Fixture dependencies installed from npm
into run directories only.

## Results

### Run set 1, 2026-10-11, large fixture

p50 / p95 in milliseconds; S5, S6 and S9 are edit to visible.

| Sequence | 0 control | 1 shared chunks | 2 + warm spare | 3 single document | 4 unbundled HMR | 5 Rspack HMR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| S1 cold | 6817 / 8474 | 3961 / 4305 | 4133 / 4607 | 4144 / 4821 | 3668 / 3768 | 3685 / 4010 |
| S1 disk-warm | 661 / 857 | 598 / 638 | 612 / 661 | 600 / 701 | 835 / 912 | 1325 / 1454 |
| **S2 paced** | **1425 / 1582** | **196 / 209** | **132 / 154** | **115 / 130** | **481 / 582** | **343 / 389** |
| S2 back to back | 1373 / 1559 | 192 / 210 | 154 / 182 | 113 / 148 | 446 / 569 | 325 / 354 |
| S3 retained | 61 / 64 | 77 / 85 | 80 / 193 | 106 / 111 | 49 / 50 | 47 / 51 |
| S4 reload | 143 / 163 | 144 / 180 | 113 / 138 | 104 / 110 | 432 / 457 | 294 / 299 |
| S5 page edit | 2255 / 2283 | 783 / 809 (7 lost) | — (12 lost) | 1151 / 1188 | 48 / 65 | 112 / 159 |
| S6 shared edit | 4511 / 6619 | 1669 / 1725 | 1736 / 1870 (5 lost) | 1348 / 1458 | 65 / 73 | 124 / 134 |
| S9 component edit | 1973 / 2057 | 793 / 1545 | 1433 / 1466 | 1414 / 1424 | 58 / 68 | 119 / 158 |

Small fixture: S2 took 251 ms for control, 57–79 ms for variants 1–3, 94 ms
for variant 4 and 114 ms for variant 5 (one sample each; only four previews to
walk). Page and shared edits took 598 and 828 ms for control, 438–567 ms for
variants 1–3, 47–49 ms for variant 4 and 620–742 ms for variant 5.

| HMR check | 0 | 1 | 2 | 3 | 4 | 5 |
| --- | --- | --- | --- | --- | --- | --- |
| S9 state kept | 0 of 10 | 0 of 10 | 0 of 10 | 0 of 10 | 10 of 10 | 10 of 10 |
| S10 module edit | reload, 4761 ms | reload, 1503 ms | reload, 1443 ms | reload, 1919 ms | in place, 196 ms | in place, 166 ms |
| S10 environment edit | reload, 4321 ms | lost | lost | lost | reload, 252 ms | document reload, 5150 ms |

Diagnostic controls against control's paced S2 of 1425 ms: D1 1316 ms, D2
1280 ms, D3 1181 ms. Readiness waits cost about 0.1 s and development React
and source maps about 0.15–0.25 s each on this fixture.

### Discovery: stale compiler output

When a source file changed while Workbench was compiling a preview, the
compiler recorded the file's new timestamp but the output reflected the old
content, so stale output was served as current until the next edit. A probe
against the shipping compiler wrote edit A, started a compile, wrote edit B
150 ms later, and received A's output from that compile and the next while the
file held B. Variants 1–3 inherited the same pattern; variants 4 and 5 use
their own watchers.

## Correctness

- Render parity: every variant and diagnostic matched control on every
  captured preview, state and size (20 of 20 small, 40 of 40 large).
- Isolation: variant 3 leaked module state in 16 of 20 small and 36 of 40
  large renders. No other variant leaked.
- Error containment: every variant showed both broken previews and loaded a
  working one. Variant 5's compile error took 35 s because the lab's preview
  module polled for a registration that a throwing entry never made; a variant
  defect, not a cost of Rspack.
- Clean console: variant 5 logged 32 exceptions on the large fixture, from
  hidden retained frames receiving hot updates they could not apply.
- Freshness: edits were lost in variants 1 and 2 and in D1, from the stale
  output defect above.

## Threats to validity

One Intel host; one run set; a synthetic fixture whose fonts and images load
locally; headless Chrome rather than the VS Code webview; the mixed fixture
and S7 unmeasured; small-fixture S2 has one sample per cell.

## Decision and follow-ups

- Rejected: variant 3 (isolation), variant 5 (clean console).
- Variants 1 and 2 load previews clearly faster than every HMR-capable variant
  (132–196 ms against 343–481 ms); the technique to combine with HMR is a
  project-wide shared build with content-hashed chunks loaded ahead into a
  spare frame.
- Variant 4 passes the HMR checks without a correctness failure but misses the
  proposed S2 target and the small-fixture reload (S4).
- Result: inconclusive pending a second run set, which did not run; the
  follow-up was the [Rspack pipeline](rspack-pipeline.md) comparison. The
  stale-output defect was to be fixed in the product regardless.
