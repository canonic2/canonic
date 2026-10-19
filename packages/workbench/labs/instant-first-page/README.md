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
([decisions](../../specs/preview-pipeline/README.md#decisions)). The owner
agreed the [stage 2 plan](#stage-2-plan) the same day. Stage 2 ran two
run sets on 2026-10-16 and is [reported](#stage-2-report): R is the leading
variant, much faster than the control from 40 pages up, but no variant meets
the agreed targets or the rule; the decision is the owner's.

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

Status: planned and agreed by the owner on 2026-10-15, after stage 1's
report and the decision to continue in parallel with the page index's
implementation, with the decisions below as listed (D2 included). Run and
[reported](#stage-2-report) on 2026-10-16, with the amendment below.

Amendment, 2026-10-16, after the smoke runs and before any run set:

- **The package was restructured during the smoke runs** (the Node server
  and worker under `server/node/`, the preview adapters under `ui/preview/`,
  the page index as the `server/` Rust package). The lab follows the new
  paths; run sets measure the restructured package.
- **The server starts the preview worker on the first preview request**, not
  at startup, since the page index shipped. The control's pre-warming
  therefore begins when the canvas asks for a page. The runner warms every
  variant with a descriptor request instead of waiting for a pre-warming
  event.
- **On `colossal`, the control's first preview request fails** after the
  Workbench server's own 120 s deadline ("Preview request timed out") while
  the worker builds every page. The runner's warm-up asks again until the
  build finishes, for at most 15 minutes. A start sample ends on the page's
  own error and records it as an error with no time; a start has no
  previous page whose error could end its wait.
- **Cold samples do not unpack the bundled runtime.** The product keeps it in
  extension storage, unpacked once; the runner's own launcher (`serve.cjs`)
  gives the server a storage folder that outlives the samples, while every
  cache in the system temporary folder stays per sample.
- **Prototype fixes** (none changes a boundary):
  - the host saves the evaluated `workbench.config.ts` with the stamps of
    the files it read, and reuses it while none changed; evaluating it on
    each start put 290 ms on the first page's path;
  - RN adds up to 8 neighbours per page asked for; its allowance had grown
    with the neighbours themselves, so it kept adding pages while idle;
  - the host exits when the process that started it does;
  - the entry parity check ignores the one difference by design, the
    snapshot path Workbench's adapters are imported from;
  - the browser launch is retried once and the retry recorded;
  - the page index's Node fallback runs the bundled Electron runtime as Node
    (\`ELECTRON_RUN_AS_NODE\`), as the host's own evaluator does; without
    it, RN's page order on \`colossal\`, the one fixture with computed
    definitions, made the host panic and restart, which showed as reloads
    and timeouts. A failed evaluator is now each definition's error, not a
    crash;
  - RN's neighbours are the opened page's group by distance in the page
    list, then the pages after it; ordering the group by list position had
    picked the group's first pages;
  - edits wait up to 120 s before they count as timeouts: on \`colossal\`,
    the control's edits did not show within 30 s.
- **D2 is left out of the run sets; its question is answered.** Removing a
  page from the compiler sends a hot update that disposes that page's
  modules in every document that loaded it. Returning to the page then
  fails in the warm document with "factory is undefined" (Rspack's
  "unexpected require … to disposed module"), so a cap that removes entries
  breaks returning to them unless the preview runtime reloads such a page.
  A probe with a cap of 3 on `large-system`, and the smoke's hot-update
  check on `large-system` and `giant`, showed it.

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

What stage 1 cannot say: what the canvas, the server, entry generation, the
browser's module loading and the hot client add, and whether hot updates
apply in a real document.

The Workbench server (`server.js`, the canvas, `workbench.yaml` and the
config answer) is the same in every variant: the shipping one, with the
page list from the Rust index as the page index implementation does. Moving
it to Rust is a planned migration in the owner's direction, not a question
for this lab; D1 measures what it adds per request as that migration's
baseline. What varies is everything behind `/_workbench/previews/`, which
the shipping server forwards to the preview worker.

### Variants

| # | Variant | Supported subset and missing contracts |
| --- | --- | --- |
| 0 | **Control.** The shipping server, worker and pipeline, unchanged | Full |
| R | **Page-only start in Rust.** The Rust host replaces the preview worker: it serves the preview routes (descriptor, page, preview module, built files, the event stream) and compiles. It starts with no page; a page becomes an entry when it is first requested. Its entry source is generated in Rust from the definition parsed as literals, the same parse the page index uses; a definition that is not all literals is evaluated by a Node subprocess with the shipping compiler, in one batch. No pre-warming | React and plain-module previews. Missing: Vue, HTML, Astro and React Native Web previews, docs lenses, project Rspack plugins, Storybook catalogs |
| RN | **R with neighbours.** After a page shows and the host has been idle 500 ms, R adds up to 8 pages next to it in the page list, the rest of its group first, one per build. A page request goes ahead of them | As R |

Diagnostic controls, not candidates:

- **N: the control without its front door.** `server.js` listens on the
  public port itself, as it does when the `workbench-server` binary is
  missing. Run with `--variants 0,N`, it is the comparison the server's
  [phase 1 acceptance](../../specs/server.md#phase-1-the-front-door) asks
  for: the front door's first-page, page-list and switching p50 against
  the server without it. Its renders are checked against variant 0's.
- **D1: request overhead.** Not a variant: in every variant, the Workbench
  server records, per request, the time from receiving it to the first
  byte, and the time it spends forwarding preview requests; in R, the host
  splits its own time into entry generation, waiting for a build and
  serving. It is the baseline for moving the Workbench server to Rust.
- **D2: an entry cap (proposed, optional).** R, with the compiler holding at
  most 20 pages: opening another removes the least recently opened one that
  no frame shows. It tests whether a cap stops S9's growth without breaking
  hot updates or moving modules.

### What the prototype adds to stage 1's host

- **HTTP:** the preview worker's routes, answered as the worker answers
  them (the same descriptor, page, preview module and event stream), on a
  loopback port the Workbench server forwards to.
- **Entry generation in Rust:** the shipping `previewEntrySource`'s output
  for a definition's adapter, states' sources, styles and environment, from
  the parsed literal definition, checked against the shipping
  `previewEntry` on every fixture page before the run set; the Node
  subprocess for the rest.
- **Pushed builds:** each finished build goes to frames as a `built` event,
  with errors per entry, as the shipping pipeline sends them.
- **Errors per entry:** each error is attributed to the entries whose chunks
  contain its module.
- **Watching:** the host watches the files of every build, the config and
  the opened definitions (C8), and rebuilds on change.
- **Binary assets** (images, fonts) served from memory.
- **Generations:** a config edit replaces the compiler with a new one holding
  the opened pages, and frames reload into a fresh warm host, as the shipping
  pipeline does.
- A definition edit regenerates that page's entry source.

The shipping canvas, preview host, preview runtime and hot client are used
as they are. Variants are overlays on a snapshot of the package, as in the
earlier labs; R's overlay points the preview service at the Rust host
instead of the worker.

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
- **D1's request parts**, from the Workbench server's and the host's own
  events.

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

Settled 2026-10-15: the Workbench server is the shipping one in every
variant, and the Rust host replaces the preview worker in R and RN; moving
the Workbench server to Rust is planned separately.

1. **RN's neighbours:** up to 8, the rest of the page's group first.
2. **D2, the entry cap of 20:** include it, or leave long sessions to S9
   alone.
3. **Docs lenses outside the subset,** with the other formats.

## Stage 2 report

Two run sets under the lab lock, each after smoke runs and the amendment
above: set 1 on 2026-10-16 06:49–12:43 UTC, set 2 13:44–22:49 UTC. Variants
0, R and RN; D2 left out, as amended. Each start cell has 3 warmups and 10
samples; each session step has 5 (S8 and S9-S4 one). Every sample waited for
a one-minute load average below 8 for at most ten minutes. In set 1 none
started above it (load 7.7 at the start, 2.1 at the end). In set 2, 23
samples started above it after the wait, most in the control's `colossal`
cells, under other builds on the host (load 37.7 at the start, 8.7 at the
end). The stage 2 runner did not record the measured source revision and
file hashes, as stage 1's did; the lab's files did not change between the
two sets. Evidence: [set 1](results/2026-10-16-stage2-set-1/summary.md),
[set 2](results/2026-10-16-stage2-set-2/summary.md), each beside its
`report.json`. Values are p50 in milliseconds by nearest rank, set 1 / set 2,
recomputed from the raw samples.

### Correctness

R and RN pass every check on every fixture they were run on, in both sets:
page list against the control's at every point (S2i, each S8 step), hot
updates with state kept (also after S9), isolation, error containment,
errors shown in their own frame, clean console, the page back after every
config edit, no module moved between shared chunks, entry and hot-client
parity, no project writes. Exceptions, none of them a defect in a variant:

- **`formats` render parity** fails for R and RN in both sets: its React
  pages use the fixture's project plugins, a missing contract of the subset
  (stage 1 found the same). Its plain-module page passes entry parity.
- **RN `large-system` render parity, set 2,** and one R `giant` S2i sample:
  Chromium timed out creating a tab ("Target.createTarget"), before the
  page was asked for. The runner retries a browser launch but not a tab.
- **The control on `colossal`:** 3–6 of 10 samples per start cell end on
  the server's own 120 s preview deadline, in set 2 only; one S4 sample did
  not show within 600 s; the first S6 edit did not show within 120 s and the
  session then stopped on a Chromium timeout, so set 2 has no control S6–S9
  on `colossal`. In set 1 one control config edit on `giant` timed out.

### Start

| Fixture | Cell | 0 | R | RN | R's own build |
| --- | --- | ---: | ---: | ---: | ---: |
| `small` | S2 first page | 1,220 / 1,234 | 390 / 372 | 370 / 374 | 65 / 62 |
| `small` | S2 page list | 211 / 207 | 220 / 213 | 209 / 211 | |
| `large-system` | S2 first page | 3,388 / 3,219 | 1,216 / 1,114 | 1,227 / 1,155 | 289 / 244 |
| `large-system` | S2 page list | 240 / 231 | 228 / 217 | 239 / 224 | |
| `giant` | S2 first page | 16,114 / 16,425 | 1,316 / 1,278 | 1,304 / 1,306 | 245 / 243 |
| `giant` | S2 page list | 408 / 415 | 416 / 409 | 412 / 414 | |
| `colossal` | S2 first page | 93,860 / 107,454 | 4,150 / 4,042 | 4,109 / 4,051 | 242 / 242 |
| `colossal` | S2 page list | 2,831 / 3,022 | 2,917 / 2,858 | 2,954 / 2,862 | |
| `small` | S1 first page, cold | 1,262 / 1,243 | 667 / 639 | 650 / 653 | |
| `large-system` | S1 first page, cold | 2,736 / 2,627 | 1,699 / 1,524 | 1,716 / 1,554 | |
| `giant` | S1 first page, cold | 10,714 / 10,886 | 1,691 / 1,671 | 1,698 / 1,700 | |
| `colossal` | S1 first page, cold | 93,607 / 115,102 | 7,338 / 7,381 | 7,542 / 7,387 | |
| `colossal` | S1 page list, cold | 6,015 / 6,580 | 6,177 / 6,231 | 6,274 / 6,101 | |

S2i and S3 follow S2 within 10% for every variant. Where R's warm first page goes (set 2,
p50 of the S2 samples):

| Fixture | Config answer | Descriptor answer | Host's build | First page |
| --- | ---: | ---: | ---: | ---: |
| `small` | 18 | 63 | 29 | 372 |
| `large-system` | 26 | 269 | 239 | 1,114 |
| `giant` | 114 | 257 | 231 | 1,278 |
| `colossal` | 793 | 828 | 481 | 4,042 |

The control's descriptor answer is its whole start: 2.4 s on `large-system`,
15 s on `giant` and 89–107 s on `colossal`, while the worker builds every
page.

### Session

| Fixture | Cell | 0 | R | RN |
| --- | --- | ---: | ---: | ---: |
| `large-system` | S4 page not yet opened | 159 / 159 | 272 / 251 | 258 / 246 |
| `large-system` | S5 walk | 140 / 144 | 231 / 217 | 327 / 251 |
| `large-system` | S5r return | 118 / 126 | 126 / 107 | 188 / 150 |
| `large-system` | S6 component, first page | 181 / 162 | 131 / 132 | 230 / 214 |
| `large-system` | S7 config edit | 2,081 / 1,962 | 4,295 / 4,324 | 4,187 / 4,240 |
| `large-system` | S9 then S4 | 146 / 140 | 347 / 340 | 334 / 338 |
| `large-system` | S9 then S6 | 197 / 181 | 251 / 232 | 248 / 230 |
| `giant` | S4 page not yet opened | 199 / 219 | 274 / 278 | 283 / 292 |
| `giant` | S5 walk | 177 / 176 | 252 / 255 | 281 / 278 |
| `giant` | S5r return | 125 / 111 | 129 / 145 | 243 / 234 |
| `giant` | S6 component, first page | 813 / 798 | 145 / 144 | 212 / 202 |
| `giant` | S7 config edit | 12,343 / 12,952 | 4,164 / 4,200 | 4,211 / 4,252 |
| `giant` | S9 then S4 | 181 / 132 | 475 / 524 | 520 / 486 |
| `giant` | S9 then S6 | 1,096 / 1,092 | 433 / 380 | 415 / 427 |
| `colossal` | S4 page not yet opened | 867 / 955 | 640 / 647 | 670 / 629 |
| `colossal` | S5 walk | 587 / 917 | 624 / 634 | 638 / 638 |
| `colossal` | S5r return | 548 / 542 | 536 / 536 | 534 / 524 |
| `colossal` | S6 component, first page | 24,000 / — | 163 / 166 | 352 / 303 |
| `colossal` | S7 config edit | 95,767 / — | 4,221 / 4,305 | 4,452 / 4,228 |
| `colossal` | S9 then S4 | 645 / — | 692 / 680 | 1,083 / 725 |
| `colossal` | S9 then S6 | 24,333 / — | 386 / 394 | 1,485 / 753 |

On `small`, S4 is 64 / 62 for the control and 72 / 63 for R. S8's add,
rename and retitle are within 30 ms of the control on `large-system` and
`giant` (one sample each; R's 92 ms retitle on `large-system` in set 2
against 26 in set 1 is one sample). Peak memory in the session, set 2: the
control 4.5 GB on `large-system`, 15.8 GB on `giant` and 20.6 GB on
`colossal`; R 1.0, 1.9 and 1.5 GB; RN 1.0, 1.8 and 1.8 GB.

### Analysis

- **The start no longer grows with the project's build.** R's warm first
  page is 4–13 times faster than the control's from `large-system` up, and
  its host builds one page in 230–480 ms on every fixture. The two sets
  agree within 10% in every R and RN start cell.
- **What remains is outside the compiler.** On `large-system` and `giant`,
  R answers the descriptor in 260–310 ms, its build included; about 0.8–0.9 s
  of the first page is after that answer, in the frame loading and running
  the page through the shipping preview host and runtime (the runner does not
  split it further). On `colossal`, the Workbench server's config answer
  takes 0.8 s and the canvas about 2 s more rendering 10,000 pages; the page
  list is 2.9 s for every variant. These are the same in every variant by
  design: the Workbench server, canvas and preview runtime are the shipping
  ones.
- **The page list is the canvas's, not the index's.** At 600 pages it is
  about 410 ms in every variant; on `small` it is 207–220 ms, just over
  200, of which the config answer is 18 ms.
- **Cold, the page list on `colossal` is the cold index** (6.0–6.6 s), and
  the first page follows it.
- **Edits are faster and do not grow with the project:** R's hot updates
  take 130–170 ms on every fixture, against 150–190 ms for the control on
  `large-system`, 800 ms on `giant` and 24 s on `colossal`.
- **A config edit costs R about 4.2 s on every fixture,** twice the
  control's on `large-system`. Samples under 10 s keep no timeline, so the
  split between R's new compiler, its first build and the frames' reload is
  not measured.
- **Opening a page is slower in R than the control on 40 and 600 pages**
  (250–280 ms against 160–220 for S4), because the control has every page
  built and R builds it on request. On `colossal` R is faster.
- **Long sessions degrade** in both variants. After S9, R's S4 is 28–35% slower
  on `large-system` and 73–88% on `giant`, and its S6 76–199% slower, the
  growth stage 1 measured in the compiler. The control's S6 grows 9–37%.
- **RN is not faster than R anywhere by 10%,** and is slower in S5, S5r and
  S6, where its neighbour builds compete with the request.

Threats to validity: one host; synthetic Acme fixtures; the Rust host's
subset (React and plain modules, without project plugins); load above 8
during part of set 2, mostly in control cells; a page list counted on the
standalone server in headless Chrome, not the VS Code webview or Studio;
the measured source not recorded by the runner.

### Result against the stage 2 decision rule

- **Correctness:** R and RN pass on their subset. The failures listed above
  are the missing project-plugin contract and browser tab timeouts.
- **Instant, first page within 500 ms (agreed):** met only on `small`
  (372–390 ms). **Not met** on `large-system` (1.1–1.2 s), `giant`
  (1.3 s) or `colossal` (4.0–4.2 s).
- **Instant, page list within 200 ms (agreed):** **not met** on any fixture
  in any variant: 209–220 ms on `small`, about 220 on `large-system`, 410 on
  `giant`, 2.9 s on `colossal`.
- **Own build time, S1 at most 25% over (proposed):** **not met;** S1 is 6
  to 30 times the page's own build.
- **Switching, S4 and S5 at most 300 ms (proposed):** met on `large-system`
  and `giant`; **not met on `colossal`** (630–650 ms).
- **No regressions:** S3, S6 and S5r on `large-system` and `colossal` pass.
  **Not met:** S7 on `large-system` (4.3 s against 2.0–2.1 s) in both sets;
  S5r on `giant` in set 2 (145 against 111, while set 1 is within 3%);
  `small` S4 in set 1 (72 against 64, while set 2 is within 2%). The last
  two are within the spread of the two sets.
- **Long sessions:** **not met** by any variant.
- **R against RN:** R wins: it has fewer moving parts and RN is not 10%
  faster in S4 or S5.

No variant meets the rule. R is the leading variant: a large gain on every
fixture from 40 pages up, with the agreed targets missed, the config edit
regressed and long sessions degrading. What the remaining time is spent on,
and whether to adopt R, change a target or plan a stage 3, is the owner's
decision.
