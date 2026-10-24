# Compiler speed

## Question

Does another bundler compile a project's TypeScript previews substantially
faster than esbuild, in the build shapes Workbench uses, with output that
renders the same? The answer decides whether Workbench keeps esbuild as its
preview compiler or evaluates a replacement in the product. It also reports
which candidates provide hot module replacement (HMR) with React Fast Refresh
usable from code, for the [preview loading](preview-loading.md) comparison.

## Status

Reported, 2026-10-11. Under the owner's amended 10% speed threshold, Rspack
meets the large-fixture speed criterion but not the small-fixture or plugin
criteria; Bun meets the speed criteria but not the capability criterion.
Esbuild was kept at the time. The [Rspack pipeline](rspack-pipeline.md)
later adopted Rspack as the preview compiler; that decision rests on its own
evidence.

## Hypothesis

Bun's bundler should be fastest on cold builds and Rolldown somewhat faster
than esbuild, following the published suites below. Rspack and Utoo should be
slower cold but fastest when their persistent caches are warm. Incremental
rebuild behavior is the least certain: each tool's in-process rebuild works
differently, and the published suites do not measure it.

Candidates were chosen on 2026-10-11 from suites maintained by tool authors on
shared CI runners; they select candidates and do not replace measurement:

- [Rolldown's benchmark suite](https://github.com/rolldown/benchmarks)
  (19,014-module app, macOS runners): Bun 1.4.2 about 0.47 s, Rolldown 1.2.13
  about 1.10 s, esbuild 0.28.2 about 1.41 s, Vite 8.3.3 about 2.39 s, Rspack
  2.2.8 about 2.55 s.
- [Rspack's build-tools suite](https://github.com/rstackjs/build-tools-performance)
  (popular-libs, without/with persistent cache): Rolldown 1.12/1.07 s, esbuild
  1.22/1.24 s, Rspack 2.31/0.76 s, Utoo 7.16/0.55 s.

| Tool | Decision | Reason |
| --- | --- | --- |
| esbuild | Control | Ships in Workbench |
| Rolldown | Variant | Faster than esbuild cold in both suites; Rollup-style plugin API |
| Bun | Variant | Fastest cold; plugin API modeled on esbuild's |
| Rspack | Variant | Fast with a warm persistent cache; webpack-compatible loaders |
| Utoo pack | Variant | Fastest with a warm persistent cache; built on Turbopack |
| Vite, Rsbuild | Not variants | Wrap Rolldown or Rspack, adding overhead |
| Farm, Parcel, Mako, webpack, Rollup | Not variants | Slower in both suites, or unmaintained |
| Turbopack | Not a variant | Runs only inside Next.js; Utoo covers its engine |
| SWC, Oxc | Not variants | Transform files but do not bundle |

## Method

**Variants.** Every variant builds the same entry modules, one per preview as
Workbench generates them (request mocks, the definition, its sources and the
project environment), with Workbench's runtime modules external. All target
ES2020 browser ESM with automatic JSX, the same `process.env.NODE_ENV`
define and asset handling, source maps and minification off.

| # | Compiler | Version |
| --- | --- | --- |
| 0 | Control: esbuild through the shipping preview compiler, unchanged | 0.28.2 |
| 1 | esbuild called directly with the lab's build specification | 0.28.2 |
| 2 | Rolldown, with a small lab plugin that turns CSS imports into style-injecting modules | 1.2.11 |
| 3 | Bun's bundler (`Bun.build`), run by the Bun binary from the run directory | 1.4.2 |
| 4 | Rspack, with its built-in SWC loader | 2.2.7 |
| 5 | Utoo pack | 1.5.22 |

Variant 1 separates Workbench's own overhead from esbuild's compile time, so
the other variants are compared against an equal build specification.
Diagnostic control D1 (variant 0 with the fixture's slow config plugin) has
no counterpart, since only esbuild runs Workbench's config plugin API.

**Build shapes.**

| ID | Shape |
| --- | --- |
| B1 | One preview built alone into a single bundle |
| B2 | Every preview as entries of one code-split build |
| B3 | After B2 in the same process, edit one page's module and rebuild |
| B4 | After B2, edit a module every page imports and rebuild |
| B5 | B2 in a new process with the persistent cache warm, after editing one page (Rspack and Utoo) |
| B6 | Development mode with HMR and Fast Refresh, edit a React component and wait for the hot update |

B3 and B4 use each tool's documented in-process rebuild (esbuild context,
Rolldown watch, Rspack memory cache, Utoo's development API); Bun and Utoo
rebuild in full. B6 runs only where HMR can be driven from code (Rspack's
watch mode with its HMR and React Refresh plugins; Rolldown's experimental
dev engine).

**Fixtures.** The [shared Acme fixture](../fixtures/labs/README.md),
`small` and `large` sizes, the same seed for every variant.

**Boundaries.** B1 and B2 run in a new process per sample with no compiler
cache; the timer starts before the compiler module is imported and stops when
output is written to disk (Rspack and Utoo write to a file system, so every
variant writes). B3 and B4 time the source write to rebuilt output on disk.
Build memory is the peak resident memory of the process tree, sampled every
100 ms. Operating-system file caches are warm and uncontrolled.

**Correctness checks.** Render parity of every preview and state in headless
Chrome against variant 0 after removing generated asset names; chunk sanity
(entries load only their chunks, locale data stays in dynamic chunks); edit
visibility after rebuilds; clean builds.

**Capability checklist**, recorded per variant: running Workbench's own
plugins, running project `workbench.config.ts` esbuild plugins, HMR with Fast
Refresh from code, aliases and defines, a Node CommonJS definition build, a
per-platform native binary and its size, and a stable programmatic API.

**Metrics.** Primary: B2 on the large fixture. Secondary: B1, B3–B6, peak
RSS, output bytes and chunk count, install size.

**Runs.** Two warmups and ten samples per variant, shape and fixture, variants
alternated in a seeded order; two run sets at least an hour apart.

**Decision rule.** A variant failing render parity or a clean build is
rejected. A replacement is recommended only if it is at least 10% faster
(30% before the amendment below) than variant 1 in B2, B3 and B4 on the large
fixture, no more than 10% slower on the small fixture, and has a plugin API
able to run Workbench's own plugins. Mixed results are inconclusive.

**Amendments, 2026-10-11.** Before any run, the owner added HMR with React
Fast Refresh as a requirement (B6 and the HMR clause). After the smoke run:
timers stop on output written to disk; Utoo writes inside the lab-owned
fixture because Turbopack refuses output outside the project root (a
capability gap); Rolldown carries the lab's CSS plugin. After sets 1 and 2,
the owner asked for a quieter re-measurement of variants 0, 1, 2, 4 and 5 one
lab at a time (sets 3 and 4), with per-sample load recorded but not gated.
After sets 3 and 4, the owner lowered the speed threshold from 30% to 10%;
both thresholds are reported.

## Conditions

macOS 15 on Intel (16 threads, 64 GB), Node 24.12, headless Chrome 154 for
render parity. Sets 1 and 2 on 2026-10-11 at Workbench `ef5a4aa`; sets 3 and 4
on the same day at `63a096f` (shipping compiler and dependencies unchanged).
Each set: 2 warmups, 10 samples per cell, no failed samples. The recorded
one-minute load average (median about 12 in set 3) includes the lab's own
compilers. Compilers and fixture dependencies were installed from npm into
run directories only.

## Results

### Sets 3 and 4, 2026-10-11

Large fixture, p50 in milliseconds for sets 3 / 4, and against esbuild
(positive is faster):

| Shape | esbuild | Rolldown | Rspack | Utoo |
| --- | ---: | ---: | ---: | ---: |
| B1 one preview | 307 / 289 | 457 / 442 (−49% / −53%) | 243 / 227 (+21% / +22%) | 1461 / 1392 |
| B2 split build | 596 / 549 | 611 / 585 (−3% / −6%) | 471 / 424 (+21% / +23%) | 7422 / 6978 |
| B3 page edit | 459 / 435 | 600 / 560 (−31% / −29%) | 135 / 145 (+71% / +67%) | 6963 / 6737 |
| B4 shared edit | 451 / 446 | 595 / 567 (−32% / −27%) | 141 / 148 (+69% / +67%) | 7250 / 6553 |
| B5 warm persistent cache | — | — | 468 / 449 | 15037 / 14312 |
| B6 hot update | — | 28 / 29 | 140 / 149 | — |

Small fixture: esbuild B1/B2 93–96 ms, Rolldown 147–159, Rspack 107–118,
Utoo 615–649; rebuilds 22–23 ms for esbuild, 115–126 Rolldown, 33–43 Rspack,
462–482 Utoo. Rspack's watch-mode rebuilds dropped from about 340 ms (sets 1
and 2) to about 140 ms on the large fixture and rose from about 16 to 33–45 ms
on the small one; the host was restarted between sets and the change is not
attributed.

### Sets 1 and 2, 2026-10-11

p50 in milliseconds, set 1 / set 2, against variant 1.

Large fixture:

| Shape | shipping | esbuild | Rolldown | Bun | Rspack | Utoo |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| B1 | 1424 / 1407 | 328 / 332 | 476 / 456 (−45% / −37%) | 145 / 139 (+56% / +58%) | 252 / 241 (+23% / +28%) | 1505 / 1463 |
| B2 | — | 625 / 600 | 634 / 613 (−1% / −2%) | 333 / 330 (+47% / +45%) | 457 / 454 (+27% / +24%) | 7322 / 7624 |
| B3 | 1251 / 1239 | 489 / 475 | 598 / 583 | 236 / 234 (+52% / +51%) | 342 / 338 (+30% / +29%) | 7287 / 7016 |
| B4 | 1239 / 1277 | 480 / 462 | 590 / 586 | 247 / 242 (+48% / +48%) | 349 / 344 (+27% / +26%) | 7637 / 6792 |
| B5 | — | — | — | — | 480 / 472 | 15945 / 15353 |
| B6 | — | — | 27 / 27 | — | 343 / 341 | — |

Small fixture:

| Shape | shipping | esbuild | Rolldown | Bun | Rspack | Utoo |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| B1 | 286 / 282 | 117 / 116 | 158 / 159 | 57 / 56 | 117 / 112 | 644 / 639 |
| B2 | — | 120 / 117 | 169 / 160 | 58 / 58 | 122 / 117 | 665 / 650 |
| B3 | 181 / 180 | 43 / 43 | 131 / 123 | 32 / 32 | 15 / 16 | 484 / 476 |
| B4 | 181 / 179 | 42 / 42 | 125 / 121 | 33 / 32 | 16 / 16 | 483 / 475 |
| B5 | — | — | — | — | 38 / 37 | 963 / 941 |
| B6 | — | — | 24 / 24 | — | 18 / 18 | — |

The shipping compiler takes about 1.1 s more than esbuild alone for one cold
large preview, and 1.24 s against 0.47 s for a rebuild: definition
evaluation, validation, entry generation, source maps and caching cost more
than any difference between compilers.

### Capabilities, 2026-10-11

| Capability | esbuild 0.28.2 | Rolldown 1.2.11 | Bun 1.4.2 | Rspack 2.2.7 | Utoo 1.5.22 |
| --- | --- | --- | --- | --- | --- |
| Runs Workbench's own plugins unchanged | Yes | No: Rollup-style hooks | No: `build.resolve()` not implemented; no in-memory entry | No: loaders and plugins need rewriting | No: webpack-style config; no documented API |
| Runs project esbuild plugins | Yes | No | Partly | No | No |
| CSS bundling | Yes | No built-in | Yes | Yes | Yes |
| Node CommonJS definition build | Yes | Yes | Yes | Yes | Not checked |
| Builds in memory | Yes | Yes | Yes | Needs an in-memory file system | No: writes inside the project root |
| URL externals (`/_workbench/…`) | Yes | Yes | Yes | Yes | No |
| HMR with Fast Refresh from code | No | Experimental dev engine | Only in its dev server | Yes | Only in its dev server |
| Native package size (darwin-x64) | 11.7 MB | 17.3 MB | 69.3 MB runtime | 50.4 MB | 68.2 MB |

Project health on 2026-10-11: none of the projects is archived; esbuild is
maintained by one person without active feature development; Rolldown,
Rspack and Bun had 100+ commits in the preceding month; Utoo had 25.

## Correctness

Every variant rendered every state of every preview the same as the shipping
compiler in all four sets (10 of 10 small, 80 of 80 large), every rebuild
contained its edit and locale data stayed in dynamic chunks. No build reported
errors; warnings were not collected. After set 1 the parity normalizer was
corrected to remove Utoo's dot-separated asset hashes; a recheck matched 80 of
80.

## Threats to validity

One Intel host; Apple silicon may rank the tools differently. Timers include
writing output to disk. The fixture is synthetic, with no config plugins in
timed cells. Rolldown carried the lab's CSS plugin, and Utoo needed rewritten
entries. Recorded load includes the lab's own compilers. Sets 1–2 and 3–4
disagree on Rspack's small-fixture rebuilds.

## Decision and follow-ups

- Original 30% threshold: no variant qualifies; Rspack reaches 21–27% in B2.
- Amended 10% threshold: Rspack meets the large-fixture speed criterion in all
  four sets (B2 21–27%, B3 29–71%, B4 26–69% faster), misses the small-fixture
  criterion in sets 3 and 4 (B1 15%, B2 20–23% slower, 14–22 ms), and does not
  meet the plugin criterion.
- Bun meets the speed criteria in sets 1 and 2 but not the capability
  criterion (`build.resolve()`, in-memory entries; a 69 MB runtime per
  platform).
- Rolldown and Utoo are slower than esbuild and are not recommended.
- For HMR with Fast Refresh from code, Rspack is the candidate with the faster
  B3/B4 and was used for the preview loading comparison's bundled-HMR variant.
- Esbuild was kept by this comparison; the later
  [Rspack pipeline](rspack-pipeline.md) decision adopted Rspack for previews
  on separate evidence.
