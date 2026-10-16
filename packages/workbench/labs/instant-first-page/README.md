# Instant first page lab

Methodology: [Workbench labs](../../specs/labs.md). Owning contracts:
[TypeScript previews](../../specs/previews.md) for discovery and the page
list, [compiler](../../specs/preview-pipeline/compiler.md) for the entry set
and start, [hot updates](../../specs/preview-pipeline/hot-updates.md) for what
must keep working, [core](../../specs/core.md) for the canvas, and
[performance](../../specs/preview-pipeline/performance.md#first-open-and-edits)
for the requirement and targets.

Status: the owner agreed the plan on 2026-10-15, with the
[decisions](#decisions-for-the-owner-before-any-run) as listed. Stage 1 ran
on 2026-10-15 and is [reported](#stage-1-report): every capability passes
and the Rust host is faster in every build cell, but two proposed stage 2
gates are missed. The owner decided on 2026-10-15 to implement the Rust
page index as measured and to continue with stage 2 in parallel
([decisions](../../specs/preview-pipeline/README.md#decisions)). The
[stage 2 plan](#stage-2-plan) awaits the owner's agreement.

Amendment, 2026-10-15, after the smoke runs and before stage 1's run set:

- **`colossal` has 2–4 sections per page.** At `large-system`'s 14–34
  sections of 5–11 KB, 10,000 pages would be about 2 GB of source. With 2–4,
  it is about 300 MB in 30,071 folders. One definition in 40 takes its title
  from `src/app/page-list.ts` and another its states, 500 in all.
- **C1 failed on stable 1.99.0, so decision 5 applies.** Rspack's crates
  enable a nightly-only feature (`regress`'s `pattern`). The host builds
  with Rspack's `nightly-2026-04-16`, installed into the lab's Cargo cache,
  and its lockfile starts from Rspack's own `Cargo.lock` at `v2.2.7`: a fresh
  resolution picked a newer `lightningcss` that the crates do not build with.
- **The Rust index has two warm checks.** `walk` reads every directory, as
  planned. `dirs` stats every known directory and reads only those whose
  modification time changed, which happens when an entry is added, removed
  or renamed in them. I2 is measured in both; I3 in `dirs`. The saved index
  is a postcard file with paths relative to the project, and the complete
  answer is saved beside it.
- **The comparison host mirrors the Rust host.** `node-host.mjs` serves the
  same line protocol over `@rspack/core` 2.2.7 with the Rust host's options
  (the stable chunk layout, the same rules and output), not the shipping
  options, so B cells differ only in the host. Both build the entries the
  shipping pipeline's `previewEntry` generates.
- **B3 holds N to N+12 entries:** each of its 13 samples adds one more page
  to the same compiler. B1 and B2 report the host's own build time and the
  time from spawning the host.
- **The Rust subset is React and plain-module adapters, without project
  plugins.** HTML previews need the pipeline's JavaScript HTML loader, like
  Vue's; they are missing contracts with Vue, Astro, React Native Web and
  project Rspack plugins.
- **C4 is checked by the hot update's content and file set**, which match
  `@rspack/core`'s for the same edit; applying it in a document is stage 2.
  **C7** checks that a broken entry reports its error and the other entries
  still build; attributing errors to an entry is the host's job, in stage 2.
- **Smoke defects fixed** (runner and prototype, none changes a boundary):
  each capability check's verdict was replaced by the build's own `ok`, so
  checks passed with build errors; both hosts lacked the pipeline's
  CSS-module default export; the host did not watch `workbench.config.ts`,
  which is outside the module graph; and the runner imported itself.

Findings from preparing the host, recorded before the run set:

- The Rust builder leaves the JavaScript parser's `import_meta` unset for a
  `web` target, and Rspack panics on its worker threads, so the build never
  returns. The host passes the parser defaults the Node binding applies.
- The CSS plugin reads `css/module` parser options only as the
  `CssAutoOrModule` variant, and the builder overwrites module-level ones;
  `namedExports: false` works only on the rule, in that variant.
- An entry dependency created before the persistent cache restores makes
  Rspack panic (“The global dependency id generator is not 0”). The lab's
  entries plugin creates each entry's dependencies on the first build that
  includes it, as Rspack's own `EntryPlugin` does.
- When a page joins, other pages' entry chunks keep their content but can
  get a new content-hash file name.
- A newly built 99 MB host binary took 0.8 s to start the first time and
  12 ms afterwards: macOS checking a new executable.

## Plan

### Question

Can a Rust preview host, using Rspack's Rust crates directly, show the first
page and the complete page list within the agreed instant targets whether a
project has 5, 600 or 10,000 pages, by starting from that page alone and a
saved page index, while edits, config edits and page switches stay within
the owner's 10% rule?

The answer decides how Workbench's preview host is built and how it starts:
in Rust, for Workbench as a VS Code extension and as part of Studio, with
Electron kept for screenshots. It also establishes what of the current
pipeline Rust can carry and what still needs a JavaScript runtime.

### Targets

Agreed by the owner on 2026-10-15, for Workbench definitions (Storybook
catalogs are out of scope), with a warm cache:

- the first page visible within 500 ms of the canvas opening;
- the complete page list (every page's title, group, states and icon)
  visible within 200 ms of the canvas opening, on first paint, not filled
  in progressively;
- with a cold cache, the first page in about its own build time.

These hold at any page count.

### Evidence so far

From the [first page lab](../first-page/README.md#report) (three run sets,
2026-10-13 to 2026-10-15, synthetic fixtures, one host, headless Chrome):

- Every cost that grew with the page count came from the start's design:
  evaluating every definition to list pages, preparing and declaring every
  entry before the first build, the canvas waiting for the whole page list,
  and, in the control, building every entry before serving a page.
- Lazy compilation passed every check and made edits about 0.25 s against
  0.9 s, with a fifth of the memory, but still declares every entry.
- With every entry built under the stable chunk layout, joining in batches,
  edits took about 4 s and config edits about 80 s on 600 pages. Adding only
  the pages that are opened was not tested.

Research for this plan, 2026-10-15, exploratory (code and documentation
reading, no builds):

- **Rspack's Rust crates are published with each release.** `@rspack/core`
  2.2.7, which the package pins, is crate version 0.102.7 (the workspace
  version at the `v2.2.7` tag). Both the `rspack` crate and
  `rspack_binding_api` say the API is not stable, does not follow semantic
  versioning, and should be pinned exactly.
- **What the crates expose:** `Compiler::builder()` (context, entries,
  devtool); `Compiler::build()`; `Compiler::rebuild(changed_files,
  deleted_files)`, which carries the previous compilation's records into the
  next, as hot updates need; replaceable input, output and intermediate file
  systems; and native crates for hot updates (`rspack_plugin_hmr`), lazy
  compilation, dynamic entries, split chunks, the SWC loader, React Refresh
  (`rspack_loader_react_refresh`), Lightning CSS, CSS, the persistent cache
  (`rspack_storage`) and a file watcher (`rspack_watcher`). The `rspack`
  crate's documentation lists its builder API as incomplete.
- **Rspack builds with a pinned nightly Rust** (`nightly-2026-04-16`).
  Whether the crates build on this repository's stable 1.99.0 is not known.
- **What stays in JavaScript today:** the Vue loader, `css-loader` and
  `style-loader`, the Astro compiler, Rspack plugins a project adds in
  `workbench.config.ts`, and evaluating definitions and the config, which are
  TypeScript programs.
- **The canvas waits for `/_workbench/config`** before it routes the address
  (`workbench/workbench.js`). That answer awaits an index that evaluates
  every definition. The page list needs each definition's `id`, `title`,
  `icon`, state IDs and labels, and sizes, which are literals in typical
  definitions.

### Hypothesis

- A Rust page index (walk, check against a saved index, parse changed
  definitions for their list fields) answers in well under 200 ms at 10,000
  pages, warm, and in about a second cold.
- A Rust host that starts with the requested page as its only entry, and
  adds an entry when a page is first opened, builds the first page in about
  its own build time at any page count, with hot updates and React Fast
  Refresh, and adds a page in under 300 ms.
- Without the Node binding between Rspack and its host, each build and
  rebuild is at least 10% faster than the same work through `@rspack/core`.

### Stages

**Stage 1, feasibility and parts.** A Rust prototype of the index and of the
compiler host, measured directly, without the canvas. It answers whether
each part works and what it costs, and it gates stage 2.

**Stage 2, end to end.** The Rust host serves the shipping canvas and
preview runtime through headless Chrome, against the shipping pipeline, on
the sequences below. Its plan is completed from stage 1's results and agreed
before any stage 2 run.

### Stage 1

#### Capabilities

Each is pass or fail, with what it took, on the `small` and `formats`
fixtures, React and HTML previews:

| # | Capability |
| --- | --- |
| C1 | The crates at 0.102.7 build on a pinned toolchain (stable 1.99.0 first, Rspack's nightly if stable fails) |
| C2 | One React preview entry builds from virtual entry sources, with the SWC loader, React Refresh and Rspack's native CSS, into memory |
| C3 | An entry is added to a running compiler and the next rebuild includes it, with no other entry rebuilt |
| C4 | An edit to a source file rebuilds and emits a hot update the shipping preview runtime's hot client can apply (checked by loading the emitted update in a page) |
| C5 | The persistent cache restores across a process restart and the warm build skips unchanged modules |
| C6 | The stable chunk layout (names by package and project folder) is expressible with the split chunks crate, and no shared module moves when entries are added |
| C7 | A compile error in one entry is reported for that entry alone |
| C8 | The file watcher reports changes to sources and to `workbench.config.ts` |

A capability that fails is a finding, not a defect to work around in
stage 2. A failure of C1 to C4 stops the lab and is reported.

#### Measurements

| ID | What | Variants |
| --- | --- | --- |
| I1 | Page index, cold: walk, check and parse every definition; the share needing evaluation is evaluated by one Node esbuild build, timed separately | Rust index; the shipping index (`preview/compiler.cjs`, read-only) |
| I2 | Page index, warm: walk and check against the saved index, nothing changed | Rust index |
| I3 | Page index, 1% of definitions retitled since the save | Rust index |
| B1 | One page's build in a new one-entry compiler, empty cache | Rust host; `@rspack/core` 2.2.7 one-entry compiler with the same options |
| B2 | The same, warm persistent cache, new process | Both |
| B3 | Add one entry to a compiler holding 1, 20 and 100 entries | Both |
| B4 | Edit a component, and a module every page shares, with 1, 20 and 100 entries | Both |

The `@rspack/core` side is a Node script in the lab with the options the Rust
host uses, so the two differ only in the host. Each index variant's output is
compared with the shipping index's.

#### Stage 1 decision rule

- Stage 2 goes ahead if C1 to C4 pass, I2 p50 is at most 50 ms on
  `colossal` (a quarter of the 200 ms list budget, leaving the rest for the
  server answer and the canvas; proposed), and B3 p50 is at most 300 ms with
  100 entries (proposed).
- The Rust host is at least 10% faster than `@rspack/core` in B1, B2, B3 or
  B4 for the result to count as a speed gain of the move; otherwise the
  move's case rests on integration and memory, reported as such.
- Capabilities that fail become stage 2's missing contracts, or reasons not
  to run it.

### Stage 2

Planned in full after stage 1's report: see [stage 2 plan](#stage-2-plan).

### Fixtures

The [shared Acme fixture](../fixtures/README.md), seed 1, with one size
added:

| Size | Shape | Used for |
| --- | --- | --- |
| `small` | 5 component previews | The case the control handles well |
| `large-system` | 40 pages, with isolation sentinels | Sessions |
| `giant` | 600 pages, the `large-system` shape | Sessions |
| `colossal` | 10,000 pages, the `large-system` shape; 5% of definitions take their title or states from an imported module, so their list fields need evaluating. New for this lab | The motivating case |
| `formats` | Every native format and a docs lens | Capabilities and correctness |

### Runs

Stage 1: three warmups and ten samples per I and B cell, on every fixture
the cell applies to, variants alternated in a seeded order; one run set,
smoked first. Stage 2: three warmups and ten samples per S1–S3 cell, five
per session sequence, two complete run sets at least an hour apart, each
smoked first. Every sample waits for the one-minute load average below 8,
at most ten minutes, and records the wait and starting load.

### Host and tools

The first page lab's host. Rspack crates pinned at 0.102.7 with a committed
`Cargo.lock`; the toolchain pinned in the lab's `rust-toolchain.toml`;
`@rspack/core` 2.2.7 and the loaders the package pins; Node 24 for the
control and the evaluation fallback; headless Chrome for stage 2. Reports
record load averages, tool versions and the measured source; no CPU model,
OS release or process lists. Apple silicon, the VS Code webview and Studio
are not measured.

### Code and caches

- The Rust prototype is a Cargo project in this lab's `host/`, outside every
  repository workspace. The lab changes no product code; product code,
  tests and packaging never build or name it.
- Build output goes to the run directory. Crate downloads and compiled
  dependencies go to one lab-owned cache under the system temporary folder,
  kept between this lab's runs so that each run does not rebuild Rspack, and
  never to the user's `~/.cargo`.
- The prototype is measurement code: formatted and free of Clippy warnings,
  without the package gates of the repository's published Rust crates.

### Budget and authorization

Network use: the fixtures' pinned npm dependencies, Rspack's crates and
their dependencies at the `Cargo.lock` versions, and, if C1 needs it, Rspack's
pinned nightly toolchain. No other external calls.

### Decisions for the owner before any run

1. **Stage 1 first,** with stage 2 planned in full from its results.
2. **Stage 2's subset:** React and HTML previews and docs lenses in Rust;
   Vue, Astro, React Native Web and project Rspack plugins left as missing
   contracts, since each needs a JavaScript loader or plugin.
3. **The JavaScript fallback** for computed list fields and the config: a
   Node subprocess in the lab. Which runtime the product uses (Node beside
   the Rust host, or an embedded engine) is a separate question.
4. **The shared Cargo cache** under the system temporary folder, kept
   between runs, instead of a fresh one per run.
5. **Rspack's nightly toolchain,** if the crates do not build on stable
   1.99.0.
6. **Switching targets:** a page not yet opened is judged against the
   proposed 300 ms, since it builds when it opens; returns to opened pages
   against the control.
7. **Cold page list:** reported without a target in stage 2; I1's proposed
   bound is about a second at 10,000 pages.
8. **The `colossal` fixture's computed share** of 5%.

## Stage 1 report

One run set, 2026-10-15 22:03–22:29 UTC, under the lab lock, after the
smoke runs and the amendment above. Each cell has 3 warmups and 10 samples;
every sample waited for a one-minute load average below 8 and none started
above it (load 10.0 at the start, 5.4 at the end, at most 10.1 after a
sample). Measured at public `6d9d0b5` and private `c2dc6a76` plus this lab's
uncommitted files, hashed in the report under `measured`. Evidence:
[report](results/2026-10-15-stage1-set-1/report.json),
[summary](results/2026-10-15-stage1-set-1/summary.md). Values are p50 / p95
in milliseconds by nearest rank, recomputed from the raw samples; p95 over
ten samples is exploratory.

### Capabilities

| # | Result |
| --- | --- |
| C1 | Pass with Rspack's `nightly-2026-04-16` and its `v2.2.7` lockfile; fails on stable 1.99.0 |
| C2 | Pass: one React page builds from virtual entry sources into memory, with React Refresh and native CSS |
| C3 | Pass: a page joins the running compiler; the other pages' modules are not rebuilt (their entry chunk keeps its content, with a new file name) |
| C4 | Pass: an edit emits a hot update with the change, with the same update files as `@rspack/core` |
| C5 | Pass: a new process restores the persistent cache |
| C6 | Pass: no shared module moved when pages joined, in C6 and in all 208 B3 samples on both hosts |
| C7 | Pass: a broken page reports its error and the other pages build |
| C8 | Pass: the watcher reports a source edit and a `workbench.config.ts` edit |

Formats: on both hosts only the plain-module adapter (`canvas/badge`)
builds. The React pages of the formats fixture need its project plugins;
Vue and HTML need JavaScript loaders; React Native Web needs its own
profile; Astro is outside the subset. These are the missing contracts the
amendment lists, confirmed.

### Page index

`shipping` is `Compiler.index()` in a new Node process; `rust` is the host's
index in a new process. Times are inside the process; with process start
they are 12–25 ms more for Rust.

| Fixture | I1 cold, shipping | I1 cold, Rust | I2 warm, `walk` | I2 warm, `dirs` | I3 1% retitled, `dirs` |
| --- | ---: | ---: | ---: | ---: | ---: |
| `small` (5) | 161 / 180 | 2.5 / 2.7 | 2.5 / 2.9 | 1.6 / 1.9 | 2.3 / 2.5 |
| `large-system` (40) | 229 / 234 | 6.1 / 6.8 | 5.1 / 5.7 | 2.6 / 2.8 | 3.9 / 4.2 |
| `giant` (600) | 913 / 959 | 38 / 39 | 30 / 32 | 8.5 / 9.0 | 14 / 14 |
| `colossal` (10,000) | 12,859 / 20,656 | 1,067 / 1,110 | 364 / 379 | 108 / 123 | 214 / 263 |

The Rust index matched the shipping index's previews in all 208 comparisons
and listed every retitled page in all 52 I3 samples. On `colossal`, cold
Rust indexing is about half evaluation (the 500 computed definitions, in one
Node esbuild build); warm `dirs` is about 45 ms loading the saved index,
50 ms checking 30,071 directories and 13 ms checking definition files.

### Builds

`rust` is the Rust host; `node` is `@rspack/core` 2.2.7 with the same
options. B1 and B2 are the host's build time for one page, in a new process,
with an empty and a warm persistent cache.

| Fixture | B1 rust | B1 node | B2 rust | B2 node |
| --- | ---: | ---: | ---: | ---: |
| `small` | 63 / 71 | 82 / 87 | 20 / 43 | 47 / 94 |
| `large-system` | 244 / 253 | 312 / 392 | 195 / 221 | 432 / 447 |
| `giant` | 243 / 262 | 319 / 426 | 192 / 205 | 432 / 440 |
| `colossal` | 241 / 250 | 315 / 325 | 188 / 197 | 432 / 449 |

From spawning the process, B1 took 259–262 ms for Rust and 516–521 ms for
Node on the three larger fixtures.

B3 adds one page to a compiler holding 1, 20 or 100 (growing by one per
sample); B4 edits a component of the first page and a module every page
shares, with the same compiler.

| Fixture | Held | B3 rust | B3 node | B4 component rust | B4 component node | B4 shared rust | B4 shared node |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `large-system` | 1 | 75 / 85 | 118 / 157 | 68 / 74 | 121 / 130 | 69 / 71 | 127 / 138 |
| `large-system` | 20 | 120 / 132 | 231 / 267 | 114 / 118 | 222 / 229 | 122 / 166 | 255 / 366 |
| `giant` | 1 | 81 / 93 | 141 / 203 | 78 / 79 | 176 / 199 | 77 / 85 | 180 / 209 |
| `giant` | 20 | 127 / 141 | 323 / 364 | 127 / 131 | 329 / 351 | 127 / 133 | 333 / 340 |
| `giant` | 100 | 322 / 342 | 909 / 955 | 317 / 341 | 1,074 / 1,144 | 304 / 321 | 897 / 991 |
| `colossal` | 1 | 63 / 76 | 96 / 137 | 64 / 68 | 116 / 120 | 63 / 68 | 120 / 168 |
| `colossal` | 20 | 100 / 115 | 218 / 252 | 101 / 104 | 222 / 235 | 100 / 102 | 221 / 255 |
| `colossal` | 100 | 234 / 242 | 717 / 763 | 238 / 251 | 678 / 753 | 238 / 248 | 682 / 710 |

### Analysis

- **One page's build does not depend on the project's size** with either
  host: B1 is 241–244 ms in Rust on 40, 600 and 10,000 pages, and B2
  188–195 ms. The first-page cost that grew with the project in the first
  page lab came from the start's design, as the hypothesis said.
- **The Rust host is faster than `@rspack/core` in every B cell**, by
  22–24% for a cold page (B1), 55–59% for a warm one (B2), 34–67% for adding
  a page and 44–71% for edits. From process spawn, a cold page shows its
  build in half the time (262 against 517 ms), most of it Node's start and
  loading `@rspack/core`.
- **The page index is 12–64 times faster in Rust when cold** and, warm,
  answers 10,000 pages in 108 ms (`dirs`). Reading every directory instead
  (`walk`) costs 364 ms there: on a warm start, checking directory stamps is
  what keeps the list inside its 200 ms budget.
- **A compiler's rebuilds grow with the pages it holds,** on both hosts:
  adding a page or editing one costs about 65–80 ms with one page held,
  100–130 ms with 20, and 235–320 ms with 100 in Rust (680–1,070 ms in
  Node). Every rebuild re-seals the whole compilation, whichever module
  changed. A session that opens many pages slows every later edit and page
  join; this is the cost the plan's S9 measures, and the first page lab's
  batched variants showed at 600 pages.

Threats to validity: one host and one run set; synthetic Acme fixtures whose
definitions are almost all literals; the Rust host covers React and plain
modules only, without project plugins; B cells time the compiler, not a page
in a browser; `@rspack/core` runs the same options as the Rust host, not the
shipping pipeline's; loads of 5–10 during samples.

### Result against the stage 1 decision rule

- **C1 to C4 pass** (C1 with the nightly toolchain, decision 5).
- **I2 p50 at most 50 ms on `colossal`** (proposed): **not met.** `dirs`
  takes 108 ms, inside the 200 ms list budget but over the quarter the gate
  set aside for it.
- **B3 p50 at most 300 ms with 100 entries** (proposed): met on `colossal`
  (234 ms), **not met on `giant`** (322 ms).
- **Speed gain of the move:** met. The Rust host is at least 10% faster than
  `@rspack/core` in B1, B2, B3 and B4 on every fixture.

Stage 1 does not meet the rule for stage 2 as written: two proposed targets
are missed. Whether to go ahead, change a target, or first reduce the index
load and the rebuild growth is the owner's decision.

## Stage 2 plan

Status: planned 2026-10-15, after stage 1's report and the owner's decision
to continue in parallel with the page index's implementation. Not agreed;
no stage 2 run has started.

### Question

End to end through the shipping canvas, does a start that builds only the
requested page, with the Rust compiler host and the Rust page index, show the
first page within 500 ms and the complete page list within 200 ms of the
canvas opening on 5, 600 and 10,000 pages, while page switches, edits and
config edits stay within the owner's 10% rule, and long sessions do not
degrade?

### Evidence so far

From stage 1, measured without a browser:

- one page's build takes the same time on 40, 600 and 10,000 pages: about
  245 ms cold and 190 ms with a warm persistent cache, in Rust;
- the Rust index answers 10,000 pages in 108 ms warm and 1.07 s cold;
- adding a page or editing one grows with the pages the compiler holds:
  about 70 ms with 1, 120 ms with 20 and 235–320 ms with 100, in Rust.

What stage 1 cannot say: what the canvas, the server, entry generation
(evaluating the page's definition), the browser's module loading and the
hot client add, and whether hot updates apply in a real document.

### Variants

| # | Variant | Supported subset and missing contracts |
| --- | --- | --- |
| 0 | **Control.** The shipping server, worker and pipeline, unchanged | Full |
| R | **Page-only start in Rust.** The shipping server and canvas, with the config's page list from the Rust index (as the page index implementation does). The preview worker is replaced by a lab worker with the same HTTP routes, which owns entry generation (the shipping `previewEntry`, one definition at a time) and drives the Rust compiler host. The host starts with no page; a page becomes an entry when it is first requested. No pre-warming | React and plain-module previews. Missing: Vue, HTML, Astro and React Native Web previews, docs lenses, project Rspack plugins, Storybook catalogs |
| RN | **R with neighbours.** After a page shows and the host has been idle 500 ms, R adds up to 8 pages next to it in the page list, the rest of its group first, one per build. A page request goes ahead of them | As R |

Diagnostic controls, not candidates:

- **D1: request overhead.** Not a variant: in R, the lab worker and the
  server record, per request, the time from receiving it to the first byte,
  split into entry generation, waiting for a build and serving. It bounds
  what moving the HTTP layer itself to Rust could save.
- **D2: an entry cap (proposed, optional).** R, with the compiler holding at
  most 20 pages: opening another removes the least recently opened one that
  no frame shows. It tests whether a cap stops S9's growth without breaking
  hot updates or moving modules.

### What the prototype adds to stage 1's host

- **Pushed builds:** the host reports each finished build on its own (one
  line per build), with errors per entry, so the worker can push `built`
  events to frames as the shipping pipeline does.
- **Errors per entry:** each error is attributed to the entries whose chunks
  contain its module.
- **Watching:** the host watches the files of every build, the config and
  the opened definitions (C8), and rebuilds on change.
- **Binary assets** (images, fonts) served from memory.
- **Generations:** a config edit replaces the compiler with a new one holding
  the opened pages, and frames reload into a fresh warm host, as the shipping
  pipeline does.
- A definition edit regenerates that page's entry source.

The lab worker uses the shipping `Pipeline` only for `previewEntry` and the
hot client source, and the shipping canvas, preview host and runtime as they
are. Variants are overlays on a snapshot of the package, as in the earlier
labs.

### Fixtures

`small` (5), `large-system` (40), `giant` (600) and `colossal` (10,000),
seed 1, as in stage 1. `formats` runs the correctness checks on its React
and plain-module pages; its other formats are the missing contracts.

### Sequences

| ID | Action | Cache state |
| --- | --- | --- |
| S1 | Start the server, open the canvas on one page at once | Cold: no saved index, empty compiler cache, new browser profile |
| S2 | The same | Warm: saved index and compiler cache, the same browser profile |
| S2i | S2 after retitling 1% of definitions while the server was stopped | Warm, index partly stale |
| S3 | Start, wait 2 s, open the canvas on one page | Warm |
| S4 | After S2's page shows, open a page not yet opened, at once | Warm |
| S5 | Walk five pages not yet opened, 1 s apart | Warm |
| S5r | Return to five pages opened earlier, in new frames | Warm |
| S6 | A component edit and a shared-module edit, on the first page and on a page opened later | Warm |
| S7 | Edit `workbench.config.ts`; the page comes back | Warm |
| S8 | Add, rename and retitle a definition while running; refresh the page list after each | Warm |
| S9 | Open 100 pages in turn (all of `large-system`'s 40), then repeat S4 and S6 | Warm |

The first page is a different page in each S1–S3 sample, from a seeded list.
Every timed step waits for a one-minute load average below 8 (at most ten
minutes, the wait and starting load recorded). S6 and S7 take five samples
per kind and page; each edit is restored and its original shown before the
next. S7 records a timeline (compiler closed and created, first build, page
back); a sample over 10 s keeps it in the report.

### Boundaries

- **First page (primary):** from the canvas document's navigation start to
  the page's own content visible, with the runner's own probe for fonts and
  images. Also reported from the server reporting its URL and from the
  server process starting.
- **Page list (primary):** from navigation start to the page list rendered
  with every expected page, counted by the runner from the canvas's page
  list against the fixture's manifest. Also the server's time to answer
  `/_workbench/config` and the index's own time.
- **Own build time:** the page's build in a one-entry Rust host with an empty
  cache (stage 1's B1), measured beside S1 on the same page.
- **Selection to visible** (S4, S5, S5r, S9) and **edit to visible** (S6,
  S7), as the [methodology](../../specs/labs.md#boundaries) defines them.
- **D1's request parts**, from the lab worker's and server's own events.

### Correctness checks

Every [methodology check](../../specs/labs.md#correctness-checks), on every
run, on the supported subset: render parity with the control's screenshots,
isolation (the `large-system` sentinels), freshness, error containment,
readiness and capture, clean console. Plus:

- the page list matches the control's (ID, title, group, states, icon,
  sizes) at every point, including after S2i and each S8 step;
- hot updates apply with React state kept after pages join, after S9 and,
  for D2, after pages are removed;
- no module moves between shared chunks during a session;
- a compile error in a page not yet opened shows in its frame, and only
  there;
- the page comes back after every config edit;
- the address's page, state and size are the ones selected.

### Metrics

- **Primary:** S2 first page and page list, from navigation start, on every
  fixture.
- **Secondary:** S1 against the page's own build; S2i; S3; S4; S5; S5r;
  S6; S7; S8; S9; D1's parts; worker and host peak memory; compiler cache and
  saved index size.

### Runs

Three warmups and ten samples per S1–S3 cell; five per session sequence.
Variants alternate in a seeded order per fixture. Two complete run sets at
least an hour apart, under the lab lock, each smoked first with one sample
per cell for every variant on every fixture.

### Decision rule

- A variant that fails a correctness check is rejected.
- **Instant (agreed targets):** S2 p50 within 500 ms for the first page and
  200 ms for the page list, from navigation start, on every fixture, in both
  run sets.
- **Own build time (proposed):** S1 p50 at most 25% over the page's own
  build, on every fixture.
- **Switching (proposed):** S4 and S5 p50 at most 300 ms on `large-system`,
  `giant` and `colossal`.
- **No regressions:** S3, S5r, S6, S7 and every `small` cell within 10% of
  the control's p50, in both run sets.
- **Long sessions:** S9's S4 and S6 within 10% of the same variant's S4 and
  S6.
- Between passing variants, the one with fewer moving parts wins unless
  another is at least 10% faster in S4 or S5. D2 informs the decision on
  long sessions; it does not win.
- Differences within the spread of the two run sets are inconclusive.

### Host and tools

As stage 1: the same host, headless Chrome through the standalone server,
the Rust host on Rspack's crates 0.102.7 with `nightly-2026-04-16`, the
shipping `@rspack/core` 2.2.7 for the control, Node 24. Reports record load
averages, tool versions and the measured source; no CPU model, OS release or
process lists. The VS Code webview, Studio and Apple silicon are not
measured.

### Budget and authorization

No network use beyond stage 1's (fixture dependencies and crates, cached).

### Decisions for the owner before any stage 2 run

1. **The server stays Node for this stage** (recommended): the shipping
   server and canvas, with the Rust index and the Rust compiler host behind
   a lab worker. It measures the shape the product takes next, keeps the
   canvas and config identical to the control, and D1 bounds what a Rust
   HTTP layer could add. The alternative, a Rust server for the canvas and
   config as stage 1's outline said, would port the config resolution and
   manifest rules the browser shares with the server, for fixtures whose
   configuration is previews alone.
2. **RN's neighbours:** up to 8, the rest of the page's group first.
3. **D2, the entry cap of 20:** include it, or leave long sessions to S9
   alone.
4. **Docs lenses outside the subset,** with the other formats.
