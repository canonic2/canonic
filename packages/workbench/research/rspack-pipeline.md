# Rspack preview pipeline

## Question

Can the proposed preview pipeline deliver every part of the performance theory
in one working implementation, on every native format, without breaking an
existing contract? The pipeline:

1. Rspack as the preview compiler, one development compiler per project.
2. Hot updates for HTML, CSS, JavaScript, React, Vue, Astro and docs pages.
3. One build per project with shared, content-hashed chunks, and lazy
   compilation so that only opened previews are built.
4. Edits pushed from the server instead of polled.
5. A warm frame or document for the next page.
6. Workbench rendering pre-warmed when a project starts.
7. Workbench's plugins and the project plugin contract rewritten for Rspack.
8. Readiness that waits only for fonts and images still loading, in a frame
   the browser renders.

The answer decides whether the pipeline is built as planned, which parts change
first, and whether a warm spare frame or a warm document is used.

## Status

Decided, 2026-10-13. The owner decided to implement the pipeline with the warm
document, its state changes re-rendering in place, and to settle the font wait
separately. The implementation is in the product and specified in the
[preview pipeline](../specs/preview-pipeline/README.md). A confirmation run through
the shipping product was inconclusive for start and edits because of host load;
navigation matched the prototype.

## Hypothesis

- The pipeline meets the proposed targets in
  [performance](../specs/system/performance.md#measured-interactions) on the large
  fixtures.
- Selecting a new page costs only its own chunks, its mount and any font or
  image it actually loads; shared code, global stylesheets and loaded fonts
  stay warm.
- Every format gets the required edit behavior; Astro is the most likely to
  fall short, because its components render on the server.
- Readiness fixes alone recover a large part of the wait, independently of
  Rspack.
- A warm document is faster than a warm spare frame for back-to-back
  navigation; whether it keeps previews isolated is not established.

Evidence before the plan: [compiler speed](compiler-speed.md) (Rspack 21–27%
faster cold and 26–71% faster on rebuilds than esbuild, HMR drivable from
code, output about three times larger); [preview loading](preview-loading.md)
(shared chunks cut a new preview from 1.4 s to 0.13–0.2 s; Rspack with HMR
showed a new preview in 0.34 s and kept state); and a 2026-10-12 probe where a
page with no web fonts took about 600 ms selection to ready, about 300 ms
loading its 5.2 MB bundle, about 130 ms in the font wait and 130–215 ms in the
image wait.

## Method

**Variants.**

| # | Variant | Supported subset |
| --- | --- | --- |
| 0 | Control: shipping behavior | Full |
| 1 | Readiness fixes only: wait only for fonts in `loading` status and incomplete images, log phase durations on the right events, mount in a frame the browser renders until promotion | Full |
| 2 | Pipeline, warm spare: items 1–8, with one prepared spare frame per project and adapter holding the runtime, shared chunks, global stylesheets and fonts | Every native format, listing unmet contracts |
| 3 | Pipeline, warm document: one long-lived document per project and adapter that changes page in place, restoring a baseline (stylesheets, listeners, timers, animation frames, extra body children, root attributes, globals, request mocks) and re-evaluating the next preview's project modules | As 2; eligible only if every isolation check passes |

Diagnostic controls: D1 variant 2 without lazy compilation (dropped, see
amendments); D2 variant 2 without pre-warming; D3 variant 2 with readiness at
mount.

**Prototype.** Overlays on a package snapshot replaced the compile worker,
server routes, preview runtime and canvas: one Rspack development compiler per
project and adapter with its persistent cache in the run directory and output
in memory; shared content-hashed chunks; one event stream per canvas for
build results (revision polling removed); hot updates for CSS, React, Vue,
HTML (reload keeping state with linked CSS replaced in place), Astro (server
re-render replacing HTML in place) and docs (Markdown re-rendered in place);
reloads for definition, environment and config edits; build errors from
Rspack's stats; pre-warming at a lower priority than requests; Workbench's
alias, Vue, HTML, Astro and React Native Web handling as Rspack loaders and
plugins, with project plugins accepting Rspack plugins and rules. Definition
and config evaluation, export, the sidebar bundle and capture stayed on
esbuild.

**Fixtures.** The [shared Acme fixture](../fixtures/labs/README.md):
`small`; `large-system` (40 pages, system fonts, extended isolation
sentinels); `large-web` (the same with heavy web fonts and a 2.5 MB photo per
page); `huge` (200 pages); `formats` (every native format, a custom adapter,
global CSS and CSS modules, and three project plugins in esbuild and Rspack
forms). Font and image responses also ran with 150 ms added per response.

**Sequences.** Start: A1 cold start, A2 hot start, A3 open as soon as the URL
is reported, A4 open after pre-warming, A5 huge against large. Navigation: N1
second page, N2 ten unopened pages paced, N3 back to back, N4 return, N5
state/inputs/size, N6 reload, N7 between adapters, N8 a page with a font not
yet loaded. Edits E1–E12: stylesheets, React, mixed modules, Vue template and
script, HTML markup and stylesheet, Astro, docs Markdown and examples,
definition/environment/config, shared modules, syntax error and fix, two saves
150 ms apart, edits with hidden retained frames. Errors and other surfaces
F1–F5.

**Boundaries.** Selection to visible starts when the canvas changes its
address and stops two animation frames after the active frame is ready and
shows the page's own element. Edit to visible starts at the file write and
stops two frames after the edit's marker shows. Start sequences count from the
server process, the reported URL or canvas navigation. Added cache states: hot
start (persistent cache from an earlier run, same browser profile) and
runtime-warm (a prepared spare or warm document).

**Correctness checks.** Render parity compares DOM structure (tag sequence,
text, image count) against control, since CSS module class names and asset
URLs differ by compiler; isolation sentinels on every N2 and N7 page;
readiness (fonts loaded and visible images complete when shown); freshness
after every edit against a fresh load; error containment within 1 s of the
build; clean console including hidden frames; no polling; no project writes
(project tree hashed); Rspack loading in the bundled Electron runtime in Node
mode; project plugins in Rspack form.

**Runs.** Stage 1 (feasibility): the formats fixture, every E and F sequence
once, gating stage 2. Stage 2 (performance): three warmups and ten samples per
cell, variants alternating per group in a seeded order, two run sets at least
an hour apart, one lab at a time, load recorded per sample.

**Decision rule.** The 10% threshold is the owner's; targets are proposed. A
variant failing a correctness check is rejected. The plan is confirmed if
variant 2 or 3 passes every check on every fixture, meets the proposed targets
on both large fixtures in both sets and is no more than 10% slower than
control on the small fixture; otherwise the failing part is named and changed.
Variant 3 is chosen only if it passes every isolation check and its N2 or N3
p50 is at least 10% faster than variant 2's on a large fixture. Readiness fixes
ship alone if variant 1 is at least 10% faster than control in N2. Lazy
compilation and pre-warming are kept if they save time without slowing any
sequence by more than 10%.

**Amendments.** During stage 1 (2026-10-12): a fixed entry set replaces lazy
compilation (finding 4, finding 9); hidden frames apply hot updates as they
arrive; Vue style blocks use `style-loader` and `css-loader`; Astro islands are
not covered (Workbench rejects them). Before stage 2: D1 dropped (identical to
variant 2); cells limited to the variants that differ in them; added latency
only on the web-font fixture; render parity by DOM structure and text;
sidebar bundle not run; variants alternate per group. Between stage 2 sets
(2026-10-13): two runner defects fixed (an undo check matched a substring, and
the freshness check compared against a copy still loading data); set 1's undo
samples for that edit are invalid and a rerun of variants 0 and 3 passed.

## Conditions

macOS 15 on Intel (16 threads, 64 GB), headless Chrome through the standalone
server. Rspack 2.2.7 with its React refresh plugin 2.0.2, Vue Loader 17.4.2,
`style-loader` 4.0.0, `css-loader` 7.1.5, `react-refresh` 0.19.0 and `memfs`
4.79.0 pinned in the lab; esbuild as the package pins. Stage 1 on 2026-10-12
at Workbench `63a096f` plus the lab's uncommitted files. Stage 2 on 2026-10-13
at `4208f0d` plus the lab's files (hashed in each report): set 1 00:31–02:13
UTC (load 7.0 at start, other agents busy), set 2 03:14–05:03 UTC (load 4.2).

## Results

### Stage 1, 2026-10-12

Variants 0, 2 and 3 on the formats fixture, every sequence once. Every page
and docs lens rendered, isolation sentinels held, broken previews showed their
errors, export stayed on esbuild, nothing was written to the project, the
worker ran in the bundled Electron runtime and the console was clean. Control
failed the no-polling check.

Edit to visible, one sample each:

| Edit | 0 control | 2 warm spare | 3 warm document |
| --- | --- | --- | --- |
| Global CSS / CSS module / Vue scoped style | 777–921 ms, state lost | 54–80 ms, hot, state kept | 52–77 ms, hot, state kept |
| React component | 843 ms, state lost | 189 ms, hot, state kept | 82 ms, hot, state kept |
| Module exporting values | 932 ms, state lost | 79 ms, hot | 78 ms, hot |
| Vue template / script | 639 / 795 ms | 52 / 52 ms (script resets state, by Vue) | 52 / 53 ms |
| HTML markup / linked stylesheet | 797 / 773 ms | 52 / 52 ms | 52 / 53 ms |
| Astro component | 801 ms | 80 ms, in place | 80 ms, in place |
| Docs Markdown / example | 752 / 961 ms, page reload | 51 / 80 ms, in place | 51 / 80 ms |
| Definition / environment | 996 / 931 ms | 106 / 106 ms, preview reload | 103 / 133 ms |
| `workbench.config.ts` | 964 ms | 375 ms, compilers replaced | 384 ms |
| Shared module with a hidden page | 937 ms | 78 ms, hidden page current | 78 ms |
| Syntax error, then fix | did not recover within 30 s | recovered in 317 ms | recovered in 266 ms |
| Two saves 150 ms apart | ends on second save | ends on second save | ends on second save |

#### Stage 1 findings

1. Rspack 2.2.7 loads and builds in the bundled Electron runtime (Electron
   44.2, Node 24.20) in Node mode, writing to an in-memory file system.
   Rspack 2 is ES modules only.
2. Rspack writes its persistent cache inside the project by default
   (`node_modules/.cache/rspack`); the pipeline points it at the system
   temporary folder. The stable `persistent` cache type takes a storage
   directory.
3. A compilation is freed when the next build starts; what serving needs is
   copied when each build finishes.
4. Hot updates do not follow a module that moves to another chunk. Adding an
   entry can move shared modules into a new split chunk, and a document that
   loaded the old layout then silently applies nothing. The entry set must
   stay fixed while documents are open.
5. Content-hashed stylesheet names break CSS hot updates; stylesheets keep
   stable names served without caching, scripts keep content-hashed names.
6. CSS modules imported as a default object need `parser: { namedExports: false }`.
7. Vue Loader 17 style blocks do not work with Rspack's native CSS; the
   classic `style-loader` and `css-loader` work with hot updates. Vue Loader
   must use Workbench's own Vue SFC compiler.
8. A warm host for shared runtimes keeps the bundler's globals, re-evaluates
   project modules per preview while keeping dependencies, restarts
   `style-loader`'s registry with them and routes build errors only to the
   preview whose graph contains them.
9. Lazy compilation was not reliable in the prototype: Rspack's middleware
   expects POST while the server forwarded GET; the first request stalled, a
   compile error did not show for a lazy entry, and pages stopped loading
   after a config change.
10. A config change replaces the compilers; documents from the old set reload
    into a fresh warm host.
11. Definitions, config, export, portable output and the sidebar bundle stay
    on esbuild; the VSIX ships both compilers.

Stage 1 was accepted on 2026-10-12 with the fixed entry set.

### Stage 2, 2026-10-13

p50 / p95 in milliseconds (single values are p50), set 1 then set 2.

Selecting a page not yet opened:

| Fixture | Sequence | 0 control | 1 readiness only | 2 warm spare | 3 warm document | D3 ready on mount |
| --- | --- | --- | --- | --- | --- | --- |
| Large, system fonts | N2 paced | 396/435, 404/427 | 411/450, 434/463 | 236/284, 238/326 | 179/257, 144/209 | 205/228, 219/243 |
| Large, system fonts | N3 back to back | 1398/1493, 1424/1508 | 1445/1615, 1491/1642 | 230/310, 226/263 | 137/164, 141/170 | 195/220, 215/273 |
| Large, web fonts | N2 paced | 1276/1309, 1272/1379 | 492/520, 481/515 | 365/992, 257/332 | 164/180, 161/194 | 237/344, 241/309 |
| Large, web fonts | N3 back to back | 2617/4963, 2485/2735 | 2348/2600, 2386/2638 | 266/402, 263/287 | 176/203, 177/190 | 237/263, 240/295 |
| Large, web fonts | N2, +150 ms per font and image | 1564, 1602 | 596, 589 | 492, 396 | 354, 303 | 228, 231 |
| Large, web fonts | N8 a new font | 2523, 2546 | 1767, 1860 | 377, 378 | 244, 225 | 362, 339 |
| Huge | N2 paced | 402, 400 | — | 233, 244 | 155, 156 | — |
| Huge | N3 back to back | 1367, 1417 | — | 231, 230 | 140, 144 | — |
| Formats | N7 between adapters | 189, 194 | 186, 197 | 186, 194 | 151, 154 | — |

Bytes per N2 selection (median): large system-font 4.98 MB in 5 scripts for
control and variant 1, 0.42 MB in 12 scripts for variant 2, 0.36 MB in 2
scripts for variant 3; large web-font 13.8 MB control, 7.8 MB variant 1,
3.0 MB variants 2 and 3 (2.5 MB of it the photo).

Other navigation:

| Fixture | Sequence | 0 | 2 | 3 |
| --- | --- | --- | --- | --- |
| Large, system fonts | N1 second page | 1377, 1431 | 321, 325 | 172, 168 |
| Large, web fonts | N1 second page | 2673, 2517 | 386, 385 | 250, 233 |
| Large fixtures | N4 retained | 55–60 | 44–52 | 94–114 |
| Large fixtures | N5 state | 50–54 | 46–49 | 143–168 |
| Large fixtures | N5 size | 17–21 | 17–24 | 16–29 |
| Large fixtures | N6 reload | 165–203 | 177–214 | 90–107 |

Start:

| Fixture | Sequence | 0 control | 2 pipeline | D2 no pre-warm |
| --- | --- | --- | --- | --- |
| Small | A3 hot, from URL | 1310, 1186 | 1154, 1206 | 1429, 1492 |
| Small | A4 after pre-warm | 352, 313 | 306, 296 | 603, 610 |
| Large, system fonts | A3 cold, from URL | 5306, 5776 | 6052, 5986 | 5780, 6459 |
| Large, system fonts | A3 hot, from URL | 1528, 1756 | 3322, 3406 | 3260, 3852 |
| Large, system fonts | A4 after pre-warm | 671, 700 | 812, 826 | 2377, 2736 |
| Huge | A3 hot, from URL | 4714, 3211 | 7764, 13350 | 7155, 9392 |
| Huge | A4 after pre-warm | 1911, 832 | 985, 2075 | 5356, 6331 |

Edits (large system-font fixture unless stated):

| Edit | 0 control | 2 warm spare | 3 warm document |
| --- | --- | --- | --- |
| E2 React component, small | 836, 874 | 60, 62 | 59, 66 |
| E2 React component (state kept in 2 and 3, lost in 0) | 1924, 1916 | 159, 160 | 151, 157 |
| E3 mixed module | 2051, 2054 | 174, 195 | 179, 178 |
| E8 definition / environment | 2063–2129 | 350–454 | 345–459 |
| E9 shared module | 2035, 2208 | 171, 203 | 169, 172 |
| Formats E1–E7 | 415–710 | 48–176 | 49–170 |
| Formats E8 config | 588–619 | 580–594 | 547–551 |

Resources after the N2 and N3 walks: canvas heap 146–155 MB for control,
268–297 MB for variant 2, 138–162 MB for variant 3; server and worker peak
memory 1.8–2.6 GB control, 1.1–1.5 GB pipeline on large fixtures, 1.7 against
2.0 GB on huge. Compile cache after a cold start: 3/16/16 MB control and
2/142/389 MB pipeline on small/large/huge.

### Confirmation run, 2026-10-13

Run through the shipping product as variant 0, with the stage 2 runner
unchanged, 16:35–17:11 UTC. Other test suites ran throughout (median load 50
during start cells, peak 528; 30 during edits), so start and edit samples are
not evidence about the product and the run is inconclusive for them.
Navigation cells ran at a median load of 11 and match the prototype's warm
document within its spread; state changes on the same page took 95–106 ms
(143–168 ms in the prototype, because the product re-renders in place) and a
reload about 60 ms. Every correctness check passed.

## Correctness

| Check | 0 | 1 | 2 | 3 | D2 | D3 |
| --- | --- | --- | --- | --- | --- | --- |
| Render parity (40 large, 15 formats renders) | reference | pass | pass | pass | — | — |
| Isolation sentinels | pass | pass | pass | pass | — | pass |
| Readiness: fonts loaded and images complete when shown | pass | fail: 26 of 26 web-font pages, both sets | pass | pass | — | exempt |
| Broken previews contained | pass | pass | pass | pass | — | pass |
| No polling | fail | fail | pass | pass | — | pass |
| Freshness after edits | pass | — | pass | pass | — | — |
| Syntax error, then fix | fail | — | pass | pass | — | — |
| Two saves 150 ms apart | pass | — | pass | pass | — | — |
| No project writes | pass, all fixtures and sets | | | | | |

Two N8 samples failed to open a browser tab and are excluded as errors.

## Threats to validity

One Intel host; headless Chrome, not the VS Code webview; background load in
both stage 2 sets; variants alternate per group, not per sample; ten samples,
so p95 is exploratory; the small fixture has four pages, so its N2 has one
sample and N3 none; the prototype is not the product.

## Decision and follow-ups

- Variant 1 is rejected: its speed on web fonts came from becoming ready
  while fonts were still loading (the font set reports `loaded` before the
  page's own text starts more downloads).
- Variants 2 and 3 pass every check. Against the proposed targets: variant 2
  misses the new-page p95 on web fonts in set 1 (992 ms); variant 3 misses the
  retained-page target on web fonts (109–114 ms) and state changes (143–168
  ms); both meet reload, edit and shared-edit targets; both miss the warm
  first page if opened at once (3.3–3.4 s; 0.81–0.83 s after pre-warming) and
  the cold first page (6.0 s, where control's 5.3–5.8 s also misses).
- Parts to change before implementation: readiness waits for fonts the page
  starts loading; the warm document re-renders in place for state, input and
  size changes and on return; the first page after a restart must not wait for
  every entry to rebuild (later studied in [first page](first-page.md)).
- Variant 3's N2 p50 is 24–55% faster than variant 2's on both large fixtures
  in both sets: the warm document is chosen, with in-place re-rendering.
- Pre-warming is kept: it saves 1.6–1.9 s on the first page when it can
  finish and slows no sequence. Lazy compilation was not measured.
- Small fixture: no sequence is consistently more than 10% slower; 20–50 ms
  cells move by more than 10% between sets in both directions.
- Owner decision, 2026-10-13: implement the pipeline with the warm document
  and in-place state changes; settle the font wait in a later comparison.
