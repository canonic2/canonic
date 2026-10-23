# Compiler speed lab

Methodology: [Workbench labs](../../specs/system/labs.md). Owning contracts:
[performance](../../specs/system/performance.md) for first-open and edit targets,
[TypeScript previews](../../specs/pages/previews.md#runtime-selection) for the
compiler Workbench ships.

Status: reported. Run sets 3 and 4 re-measured Rolldown, Rspack and Utoo on
2026-10-11. Under the owner's amended speed threshold, Rspack meets the
large-fixture speed criterion. The owner has yet to decide whether the
small-fixture and plugin criteria still apply.

Amendment, 2026-10-11, after run sets 3 and 4: the owner lowered the decision
rule's speed threshold from 30% to 10%, judging a 10% gain enough to justify a
change. The owner made this change after seeing the results. All four sets are
re-scored against it below, and the result under the original 30% threshold
is kept beside it.

Amendment, 2026-10-11, after run sets 1 and 2:

- The owner asked to re-measure Rolldown, Utoo and Rspack against esbuild one
  lab at a time on a quiet machine. At 13:50 UTC the host showed a backup
  agent at about 40% CPU and a one-minute load average of 4–6 on 16 threads.
  Run sets 1 and 2 recorded load only at their end.
- Run sets 3 and 4 measure variants 0, 1, 2, 4 and 5 (shipping and direct
  esbuild, Rolldown, Rspack, Utoo) with the same plan, fixtures, shapes and
  samples. Bun is not re-measured; its result in sets 1 and 2 is unchanged by
  this amendment.
- A lock lets only one lab run at a time, and each sample records the load
  average at its start. The runner can hold
  each sample until the load is below a threshold; the owner chose not to
  gate on load for sets 3 and 4 (threshold 16, one per hardware thread), so
  load is recorded rather than controlled.
- Sets 3 and 4 decide for the re-measured variants. Sets 1 and 2 stay in
  `results/` as evidence of the earlier conditions.
- Project health was checked on 2026-10-11 before the re-measurement; see
  [Project health](#project-health).

Amendment, 2026-10-11, before any run: the owner added hot module replacement
with React Fast Refresh as a requirement
([performance](../../specs/system/performance.md#edits-apply-in-place)). Shape B6,
the HMR capability item and the decision rule's HMR clause were added for it.

Amendment, 2026-10-11, after the harness's smoke run and before any run set:

- Timers for B1, B2, B5 and rebuilds stop when output is written to disk,
  not when it is in memory. Rspack and Utoo write to a file system and would
  need an extra in-memory file system package to do otherwise; writing for
  every variant keeps the boundary equal.
- Utoo writes inside the lab-owned fixture, because Turbopack refuses output
  outside the project root. Workbench builds in memory and writes nothing to
  projects, so this is recorded as a capability gap.
- Rolldown has no built-in CSS bundling. The lab gives it a small plugin
  that turns CSS imports into modules injecting their stylesheet, with
  `url()` assets emitted; the plugin's cost is inside Rolldown's timings.
- Build memory is the peak resident memory of the whole process tree,
  sampled every 100 ms, because esbuild compiles in a separate process.
- B6 is measured for Rspack (watch mode with its HMR and React Refresh
  plugins) and Rolldown (its experimental dev engine). Bun's and Utoo's HMR
  run only inside their own dev servers, so B6 reports them as not drivable
  from code.

## Plan

### Question

Does another bundler compile a project's TypeScript previews substantially
faster than esbuild, in the build shapes Workbench uses, with output that
renders the same? The answer decides whether Workbench keeps esbuild as its
preview compiler or evaluates a replacement in the product.

### Hypothesis

Bun's bundler should be fastest on cold builds and Rolldown somewhat faster
than esbuild, following the published suites below. Rspack and Utoo should be
slower cold but fastest when their persistent caches are warm. Incremental
rebuild behavior is the least certain: each tool's in-process rebuild works
differently, and the published suites do not measure it.

### Candidates considered

Researched on 2026-10-11. Published numbers come from suites maintained by
tool authors on shared CI runners, so they choose candidates and do not
replace this lab's measurements.

- [Rolldown's benchmark suite](https://github.com/rolldown/benchmarks),
  updated 2026-10-10, 19,014-module app on macOS runners: Bun 1.4.2 about
  0.47 s, Rolldown 1.2.13 about 1.10 s, esbuild 0.28.2 about 1.41 s, Vite
  8.3.3 about 2.39 s, Rspack 2.2.8 about 2.55 s, Rsbuild about 3.13 s, Rollup
  about 416 s.
- [Rspack's build-tools suite](https://github.com/rstackjs/build-tools-performance),
  2026-09-23, production builds of its popular-libs case without and with a
  persistent cache: Rolldown 1.12 s / 1.07 s, esbuild 1.22 s / 1.24 s, Vite
  1.37 s / 1.41 s, Rspack 2.31 s / 0.76 s, Farm 5.74 s / 1.96 s, Utoo
  7.16 s / 0.55 s, webpack 7.32 s / 2.22 s, Rollup 10.6 s / 10.6 s. Its
  ui-components case puts Parcel at 33.7 s without a cache.

| Tool | Decision | Reason |
| --- | --- | --- |
| esbuild | Control | Ships in Workbench |
| Rolldown | Variant | Faster than esbuild cold in both suites; Rollup-style plugin API |
| Bun | Variant | Fastest cold in Rolldown's suite; plugin API modeled on esbuild's. Installed from npm into the run directory, not globally |
| Rspack | Variant | Second-fastest with a warm persistent cache; webpack-compatible loaders |
| Utoo pack | Variant | Fastest with a warm persistent cache; built on Turbopack. Its npm package documents no Node API, so the lab records it as not run if it cannot be driven from its published types |
| Vite | Not a variant | Its production build runs Rolldown, adding overhead |
| Rsbuild | Not a variant | Wraps Rspack, adding overhead |
| Farm | Not a variant | About 4–5× slower than esbuild cold in Rspack's suite, and slower than Rspack and Utoo cached; no stable release since 1.7.11 (2025-08-04) |
| Parcel | Not a variant | Slowest cold build in Rspack's suite and its highest memory use |
| Mako | Not a variant | Built for the Umi framework; no release since 0.11.15 (2025-11-06) |
| webpack, Rollup | Not variants | Slower than every variant in both suites |
| Turbopack | Not a variant | Runs only inside Next.js; Utoo pack covers its engine |
| SWC, Oxc | Not variants | Transform files but do not bundle |

Compile time matters to Workbench on first open after a cache miss and after
edits. The exploratory investigation recorded in the
[preview loading lab](../preview-loading/README.md#hypothesis) found cold
per-preview compiles of 2.7–11 s on a selected large project, and cached
compiles of 12–26 ms. That evidence predates this plan and decides nothing.

### Variants

Every variant builds the same entry modules: one per preview, as Workbench
generates them (request mocks, the definition, its sources and the project
environment), with Workbench's runtime modules left external. All variants
target ES2020 browser ESM with automatic JSX, the same `process.env.NODE_ENV`
define, the same asset handling for fonts and images, linked source maps off,
and minification off.

| # | Compiler | Version |
| --- | --- | --- |
| 0 | **Control.** esbuild through Workbench's shipping `Compiler` (`preview/compiler.cjs`), unchanged | 0.28.2, as pinned by the package |
| 1 | esbuild called directly with the lab's build specification | 0.28.2 |
| 2 | Rolldown | 1.2.11 |
| 3 | Bun's bundler (`Bun.build`), run by the Bun binary from the run directory | 1.4.2 |
| 4 | Rspack, with its built-in SWC loader | 2.2.7 |
| 5 | Utoo pack | 1.5.22 |

Versions are the latest published at least two weeks before the plan. Variant
1 separates Workbench's own overhead (definition evaluation, validation, entry
generation) from esbuild's compile time, so the other variants are compared
against an equal build specification. Bun runs in its own runtime rather than
Node, so its builds run in a Bun process with the same timing boundaries.

Diagnostic control, not a candidate:

- **D1:** variant 0 with the fixture's slow config plugin enabled, to measure
  plugin-bound cost. Only esbuild runs Workbench's config plugin API, so D1
  has no counterpart in other variants.

### Build shapes

| ID | Shape |
| --- | --- |
| B1 | One preview built alone into a single bundle, as Workbench ships |
| B2 | Every preview of the project as entries of one code-split build |
| B3 | After B2 in the same process, edit one page's own module and rebuild |
| B4 | After B2 in the same process, edit a module every page imports and rebuild |
| B5 | B2 in a new process with the tool's persistent cache warm from an earlier B2, after editing one page's module |
| B6 | In development mode with HMR and React Fast Refresh on, edit one React component and wait for the tool's hot update for it |

B3 and B4 use each tool's documented in-process rebuild: an esbuild build
context, Rolldown's watch or incremental mode, Bun's watch mode, an Rspack
compiler with its memory cache, and Utoo's development project API. A tool
without one rebuilds in full, and the report says so. B5 applies only to tools
with a persistent cache (Rspack and Utoo); it models a worker restart after a
change, which Workbench's own output cache cannot serve.

B6 applies to tools with built-in HMR: its timer runs from the source file
write until the tool has produced the hot update for that module, measured in
the tool's own process. Applying the update in a browser is the
[preview loading lab](../preview-loading/README.md)'s S9. esbuild has no HMR,
so variants 0 and 1 report B6 as unsupported; Workbench would have to supply
HMR itself around esbuild, as the preview loading lab's variant 4 does.

### Fixtures

The [shared Acme fixture](../fixtures/README.md), small and large sizes, with
the same seed for every variant. The large fixture is the motivating case;
the small one is a case esbuild already handles well. A selected local project
is not used: its config plugins only run under esbuild.

### Boundaries and cache states

- B1 and B2 run in a new process per sample with no compiler cache: no
  esbuild context, no Rolldown cache, persistent caches disabled or emptied. The
  timer starts before the compiler module is imported, so its loading cost is
  included, and stops when output is written to the run directory.
  Operating-system file caches are warm and uncontrolled.
- B3 and B4 time the rebuild only, from the source file write to rebuilt
  output on disk, in a process that has just built every preview with the
  same builder.
- Variant 0's B1 timer covers `Compiler.compile` for one preview, which
  includes definition evaluation, with its disk cache redirected to the run
  directory and empty.

### Correctness checks

- **Render parity:** every build's output for every preview is mounted in
  headless Chrome through a minimal host page with the fixture's request
  mocks, and its rendered HTML is compared with variant 0's after removing
  generated asset names. Any difference rejects the variant for that fixture.
- **Chunk sanity (B2):** every entry loads with only its own chunks, and
  locale data stays in dynamic chunks rather than the entry.
- **Edit visibility (B3, B4):** the rebuilt output contains the edit, and an
  unedited page's output is unchanged apart from chunk names.
- **Clean build:** no errors; warnings are recorded and compared.

### Capability checklist

Recorded per variant, not timed, because a faster compiler that cannot do
these cannot replace esbuild without other work:

- a plugin API that can run Workbench's own plugins (the `@canonic2/workbench`
  alias, the Vue single-file component and HTML loaders, Astro resources);
- whether project `workbench.config.ts` plugins written for esbuild's API
  could run, or would need a new plugin contract;
- hot module replacement and React Fast Refresh: built in, through a
  documented plugin, or missing; and whether either can be driven from code
  without the tool's own dev server;
- aliases, defines, `resolveExtensions`, deduplicated packages;
- building definitions for Node in CommonJS with packages external;
- a native binary that can be bundled per platform in the VSIX, and its size;
  for Bun, the runtime binary the worker would have to launch;
- a documented, stable programmatic API.

### Metrics

- **Primary:** B2 time on the large fixture.
- **Secondary:** B1, B3, B4, B5 and B6 times; peak RSS of the build process; output
  bytes and chunk count; package install size.

### Runs

Two warmup samples, then ten samples per variant, shape and fixture, with
variants alternated in a seeded order. Two run sets, at least an hour apart,
with no other heavy work on the host.

### Decision rule

- A variant that fails render parity or the clean-build check on any fixture
  is rejected.
- A replacement for esbuild is recommended for product evaluation only if it
  is at least 10% (30% before the amendment) faster than variant 1 in B2, B3 and B4 on the large
  fixture (B5 is reported but does not decide), is not slower than variant 1 by more than 10% on the small
  fixture, and has a plugin API able to run Workbench's own plugins.
- Faster in some shapes but not all, or within the spread of the two run
  sets: inconclusive, reported per shape.
- Otherwise Workbench keeps esbuild.
- Separately from speed, the report states which candidates provide HMR with
  React Fast Refresh usable from code, and their B6 times. The preview loading
  lab's variant 5 uses this to pick its bundler. Whether Workbench keeps
  esbuild with its own HMR layer or adopts a bundler with built-in HMR is
  decided by the preview loading lab, using both labs' results.

### Host and tools

macOS 15 on Intel (16 threads, 64 GB). Apple silicon and other
platforms are not measured. Node 24; headless Chrome through the package's
DevTools driver in `scripts/chrome.cjs` for render parity. Compiler and
fixture dependencies are pinned in the lab and installed into the run
directory.

Budget and authorization: installing the pinned compilers and fixture
dependencies from the npm registry into run directories, authorized by the
owner on 2026-10-11. Nothing is installed into the package or any project.

## Running the lab

```sh
node labs/compiler-speed/run.mjs --source-revision <measured commit> --results labs/compiler-speed/results/<date>-<label>
```

`--smoke` takes one sample per cell with no warmups, to check the harness;
its numbers are not evidence. `--fixtures`, `--variants` and `--shapes`
restrict a run, and `--keep` keeps the generated projects and builds.

The runner creates a run directory under the system temporary folder,
installs the pinned compilers (`tools/package.json` and its lockfile) and the
[shared Acme fixture](../fixtures/README.md) there, and captures the entry
modules Workbench generates (`capture-entries.cjs`). Each sample runs in a new
process: `sample.mjs` for Node-hosted compilers and `bun-builder.mjs` under
the Bun binary from the run directory. `builders.mjs` holds each compiler's
configuration. `parity.mjs` renders every preview and state in headless
Chrome against the shipping compiler's output (`shipping-outputs.cjs`). The
report records host conditions, tool versions, the given source revision and
hashes of the measured and lab files.

## Report

### Re-measurement (run sets 3 and 4)

Run set 3 at 13:54 UTC and set 4 at 15:57 UTC on 2026-10-11, at Workbench
commit `63a096f` (the shipping compiler and its dependencies are unchanged
from `ef5a4aa`), one lab run at a time under the lab lock, with 2 warmups and
10 samples per cell and no failed samples. Evidence:
[set 3](results/2026-10-11-set-3/summary.md), [set 4](results/2026-10-11-set-4/summary.md).

Large fixture, p50 in milliseconds for sets 3 / 4, and against esbuild:

| Shape | esbuild | Rolldown | Rspack | Utoo |
| --- | ---: | ---: | ---: | ---: |
| B1 one preview | 307 / 289 | 457 / 442 (−49% / −53%) | 243 / 227 (+21% / +22%) | 1461 / 1392 |
| B2 split build | 596 / 549 | 611 / 585 (−3% / −6%) | 471 / 424 (+21% / +23%) | 7422 / 6978 |
| B3 page edit | 459 / 435 | 600 / 560 (−31% / −29%) | 135 / 145 (+71% / +67%) | 6963 / 6737 |
| B4 shared edit | 451 / 446 | 595 / 567 (−32% / −27%) | 141 / 148 (+69% / +67%) | 7250 / 6553 |
| B5 warm persistent cache | — | — | 468 / 449 | 15037 / 14312 |
| B6 hot update | — | 28 / 29 | 140 / 149 | — |

On the small fixture, esbuild's B1 and B2 took 93–96 ms, Rolldown 147–159,
Rspack 107–118 and Utoo 615–649; rebuilds took 22–23 ms for esbuild, 115–126
for Rolldown, 33–43 for Rspack and 462–482 for Utoo. Every variant matched
the shipping compiler's render on every preview and state (10 of 10 and 80
of 80 in both sets), and every rebuild contained its edit.

What changed from sets 1 and 2:

- esbuild, Rolldown and Utoo cold builds are within a few percent of the
  earlier sets, and all small-fixture builds are 15–40% faster. The
  rankings did not change.
- Rspack's watch-mode rebuilds and hot updates dropped from about 340 ms to
  about 140 ms on the large fixture and rose from about 16 ms to 33–45 ms on
  the small one; sets 3 and 4 agree with each other. These timers include
  the time Rspack's watcher takes to notice the edit, and the host was
  restarted between set 2 and set 3; the lab cannot attribute the change.
- The recorded one-minute load average (median about 12 in set 3) counts the
  lab's own multi-threaded compilers, so it does not measure background load
  alone.

Result against the decision rule, from sets 3 and 4:

- With the original 30% threshold, Rspack misses the rule (21–23% in B2).
- With the amended 10% threshold, Rspack meets the large-fixture speed
  criterion in all four sets: B2 21–27%, B3 29–71% and B4 26–69% faster than
  esbuild.
- It misses the small-fixture criterion in sets 3 and 4: B1 15% and B2
  20–23% slower than esbuild (14–22 ms), and rebuilds 11–21 ms slower. In
  sets 1 and 2 its small-fixture B2 matched esbuild and its rebuilds were
  faster, so the two pairs of sets disagree. Rspack is faster than the
  shipping compiler in every small-fixture shape: B1 107–111 ms against
  179 ms, and rebuilds 33–43 ms against 119–121 ms.
- It does not meet the plugin criterion. Workbench's own plugins, and the
  esbuild plugins that `workbench.config.ts` accepts, would have to be
  rewritten for Rspack's loader and plugin API.
- Rolldown and Utoo are slower than esbuild. Bun's result from sets 1 and 2
  is unchanged.
- The owner decides whether the small-fixture and plugin criteria stand.
  Rspack remains the bundler for the preview loading lab's variant 5.

### Run sets 1 and 2

Two run sets on 2026-10-11 at Workbench commit `ef5a4aa`, macOS 15 on Intel
(16 threads, 64 GB), Node 24.12, Chrome 154: set 1 at 06:30
UTC and set 2 at 08:09 UTC, each with 2 warmups and 10 samples per cell and
no failed samples. The host also ran an idle VS Code with the Workbench
extension. Evidence: [set 1](results/2026-10-11-set-1/summary.md),
[set 2](results/2026-10-11-set-2/summary.md) and the
[set 1 parity recheck](results/2026-10-11-set-1-parity-recheck/summary.md).

### Results

p50 in milliseconds, set 1 / set 2. Percentages are against variant 1
(esbuild called with the same build specification); positive is faster.

Large fixture:

| Shape | shipping | esbuild | Rolldown | Bun | Rspack | Utoo |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| B1 one preview | 1424 / 1407 | 328 / 332 | 476 / 456 (−45% / −37%) | 145 / 139 (+56% / +58%) | 252 / 241 (+23% / +28%) | 1505 / 1463 |
| B2 split build | — | 625 / 600 | 634 / 613 (−1% / −2%) | 333 / 330 (+47% / +45%) | 457 / 454 (+27% / +24%) | 7322 / 7624 |
| B3 page edit | 1251 / 1239 | 489 / 475 | 598 / 583 (−22% / −23%) | 236 / 234 (+52% / +51%) | 342 / 338 (+30% / +29%) | 7287 / 7016 |
| B4 shared edit | 1239 / 1277 | 480 / 462 | 590 / 586 (−23% / −27%) | 247 / 242 (+48% / +48%) | 349 / 344 (+27% / +26%) | 7637 / 6792 |
| B5 warm persistent cache | — | — | — | — | 480 / 472 | 15945 / 15353 |
| B6 hot update | — | — | 27 / 27 | — | 343 / 341 | — |

Small fixture:

| Shape | shipping | esbuild | Rolldown | Bun | Rspack | Utoo |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| B1 | 286 / 282 | 117 / 116 | 158 / 159 | 57 / 56 | 117 / 112 | 644 / 639 |
| B2 | — | 120 / 117 | 169 / 160 | 58 / 58 | 122 / 117 | 665 / 650 |
| B3 | 181 / 180 | 43 / 43 | 131 / 123 | 32 / 32 | 15 / 16 | 484 / 476 |
| B4 | 181 / 179 | 42 / 42 | 125 / 121 | 33 / 32 | 16 / 16 | 483 / 475 |
| B5 | — | — | — | — | 38 / 37 | 963 / 941 |
| B6 | — | — | 24 / 24 | — | 18 / 18 | — |

Per-cell p95, maximum, process-tree memory and output sizes are in the
summaries. Rebuilds use esbuild's build context, Rolldown's watch mode with
incremental builds, Rspack's watch mode and a full rebuild for Bun and Utoo,
as each report's `incremental` field records.

### Correctness

Every variant rendered every state of every preview the same as the shipping
compiler in both sets: 10 of 10 on the small fixture and 80 of 80 on the
large one (Utoo's set 1 result after the normalizer correction below). Every
rebuild contained its edit. Locale data stayed in dynamic chunks, never in an
entry. No build reported errors; warnings were not collected.

### Analysis

- The two sets agree within a few percent in every cell, so differences of
  more than 10% between tools are well outside run-to-run variation on this
  host.
- Bun is the fastest bundler in every cold and rebuild shape, about twice as
  fast as esbuild, as its published benchmarks suggested. Its rebuilds are
  full builds, and still beat every incremental rebuild here.
- Rspack's watch-mode rebuilds are fastest on the small fixture and 26–30%
  faster than esbuild's rebuilds on the large one; its cold builds are
  24–28% faster. Its development output is three times the size.
- Rolldown matches esbuild on the split build and is slower everywhere else
  on this host, contrary to its published benchmark on CI runners.
- Utoo is an order of magnitude slower without a cache, and its persistent
  cache made a cold rebuild after an edit slower still (about 15 s at
  3.4 GB), contrary to the published cached result.
- The shipping compiler takes about 1.1 s more than esbuild alone for one
  cold preview (1.4 s against 0.33 s), and its rebuild after an edit takes
  1.24 s against 0.47 s for an esbuild context. That cost is Workbench's own
  work around the build (definition evaluation, validation, entry generation,
  source maps and caching) and is larger than any difference between
  compilers.
- Threats to validity: one Intel host; Apple silicon may rank the tools
  differently. Timers include writing output to disk. The fixture is
  synthetic, with no config plugins in timed cells. Rolldown carried the
  lab's CSS plugin, and Utoo needed rewritten entries.

### Result against the decision rule

- Bun meets the speed criteria: at least 30% faster than esbuild in B2, B3
  and B4 on the large fixture in both sets, and faster on the small fixture.
  It does not meet the capability criterion as written: `build.resolve()` is
  not implemented, and Workbench's project resolver depends on it; there is
  also no in-memory entry. **Not recommended under the agreed rule.** It is
  the only candidate that would justify rework, if `build.resolve()` lands
  or the resolver is rewritten around `Bun.resolveSync`, and the 69 MB
  runtime per platform is acceptable.
- Rspack misses the original 30% threshold in B2 (24–27%) and B4 (26–27%),
  so it is not recommended under that threshold. It meets the amended 10%
  threshold, and its small-fixture B2 and rebuilds are within 10% of
  esbuild or faster in these two sets.
- Rolldown and Utoo are slower than esbuild: **not recommended.**
- **Workbench keeps esbuild.**
- For HMR with React Fast Refresh: Rspack (343 ms hot update on the large
  fixture) and Rolldown's experimental dev engine (27 ms) can be driven from
  code. Among them Rspack has the faster B3 and B4, so it is the bundler for
  the preview loading lab's variant 5.

Decision: pending the owner's record.

### Project health

Checked on 2026-10-11 from GitHub, npm and the projects' announcements. None
is archived or abandoned.

| Project | Ownership and status | Commits, 2026-09-11 to 10-11 | Latest release |
| --- | --- | --- | --- |
| esbuild | One maintainer; its FAQ states maintenance and periodic releases without active feature development | None since 2026-08-09 | 0.28.2, 2026-08-08 |
| Rolldown | VoidZero, acquired by Cloudflare in June 2026, which committed to keeping it MIT-licensed; 1.0 stable since May 2026; Vite 8's bundler | 100+, 13 authors | 1.2.13, 2026-10-07; 14 releases in three months |
| Rspack | ByteDance; 2.0 in April, 2.2 in August 2026 | 100+, 20 authors | 2.2.8, 2026-09-28; 16 releases in three months |
| Bun | Acquired by Anthropic in December 2025, which stated it remains open source and MIT-licensed with continued development for all JavaScript developers | 100+, 5 authors | 1.4.3, 2026-10-10 |
| Utoo | Small team; used by default in Ant Design Pro v6 | 25, 4 authors | 1.5.25, 2026-10-10; 36 releases in three months |

### Capability checklist

Recorded on 2026-10-11 from each tool's published types and the lab's runs.
Bun's API facts were probed by running it (`bun-capabilities.mjs`, stored in
each report under `capabilities`).

| Capability | esbuild 0.28.2 | Rolldown 1.2.11 | Bun 1.4.2 | Rspack 2.2.7 | Utoo 1.5.22 |
| --- | --- | --- | --- | --- | --- |
| Runs Workbench's own plugins unchanged | Yes (shipping) | No: Rollup-style hooks; every Workbench plugin needs rewriting | No: `onResolve`, `onLoad` and namespaces work, but `build.resolve()` throws "not implemented yet" (Bun issue 2771), and the project resolver uses it; no in-memory `stdin` entry | No: webpack loaders and plugins; rewrite | No: webpack-style config; rewrite; no documented API |
| Runs project `workbench.config.ts` esbuild plugins | Yes | No | Partly (no `build.resolve`) | No | No |
| CSS bundling | Yes | No built-in; the lab added a plugin | Yes | Yes (`experiments.css`) | Yes |
| Definition build for Node as CommonJS, packages external | Yes | Yes | Yes (probed) | Yes | Not checked |
| Builds in memory | Yes | Yes (`generate`) | Yes (probed) | Needs an in-memory file system package | No: writes inside the project root only |
| URL externals (`/_workbench/…`) | Yes | Yes | Yes | Yes | No: "/" imports resolve from the project root; the lab rewrote entries to bare names |
| HMR with React Fast Refresh driven from code | No | Experimental dev engine, hidden from its docs | Only inside its dev server | Yes (watch mode, HMR and React Refresh plugins) | Only inside its dev server |
| Native package size (darwin-x64) | 11.7 MB | 17.3 MB | 69.3 MB (a runtime the worker would launch) | 50.4 MB | 68.2 MB |

### Harness corrections

- After run set 1, the render-parity normalizer was corrected: it removed
  hashes that follow a hyphen in asset names but not Utoo's, which follow a
  dot. Set 1's 80 large-fixture Utoo differences were all asset names; the
  recheck (`results/2026-10-11-set-1-parity-recheck`) matched 80 of 80.
- Also after set 1, the memory column falls back to a sample's own peak when
  it finished before the first 100 ms process-tree reading (Bun on the small
  fixture read 0 MB in set 1). Timing code did not change between sets.
