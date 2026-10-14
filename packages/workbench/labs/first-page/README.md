# First page lab

Methodology: [Workbench labs](../../specs/labs.md). Owning contracts:
[compiler](../../specs/preview-pipeline/compiler.md) for the entry set and
start, [warm runtime](../../specs/preview-pipeline/warm-runtime.md) for
pre-warming, [hot updates](../../specs/preview-pipeline/hot-updates.md) for
what must keep working, and [performance](../../specs/preview-pipeline/performance.md#measured-interactions)
for the targets.

Status: the owner agreed the plan on 2026-10-13. Run set 1 finished on
2026-10-14; run set 2 has not run.

Amendment, 2026-10-13, before any run: the Rspack pipeline lab's confirmation
run was spoiled by other work on the host (load averages up to 528). Each
sample here waits until the one-minute load average is below 8, for at most
ten minutes, and records the load it started at. A sample that starts above
the threshold after the wait is kept and marked.

Amendment, 2026-10-13, after the smoke runs and before any run set:

- **Warm samples share one warm cache.** S2 and S3 start from one persistent
  cache per variant and fixture, built once and kept warm by each S2 sample's
  full build. A cache per sample would add a full build of up to 600 pages
  to every sample. S1 still starts from an empty cache each time, and closes
  as soon as its page shows.
- **Prototype fixes from the smoke runs**, none of which changes a boundary:
  - the worker lists the project once when the server and pre-warming ask
    together;
  - a page asked for before its compiler exists is in that compiler's first
    build;
  - variant 1 adds the rest of the project only after the first page is
    served, as the plan says;
  - built files are served at once while later batches build;
  - variant 3 describes its pages through its own compiler.
- **The moved-module check counts shared chunks only.** A module copied into
  each page's own chunk has not moved when a page is added.

Amendment, 2026-10-14, after run set 1 and before run set 2: run set 1
showed that listing the project's definitions, which every variant does
before its first page, grows with the page count. Each definition had its own
esbuild build. The product lists them in one build since then
([page listing](../../specs/preview-pipeline/performance.md#page-listing)), and
every variant's copy of the compiler carries that change. Run set 2 therefore
measures a different listing than run set 1: set 1 stays the record of the
cost, and the decision rule is applied to set 2.

## Plan

### Question

How can the first page after Workbench starts show in about the time that
page's own build takes, whatever the number of pages in the project, while
hot updates keep working for every open page?

The answer decides how the [preview pipeline](../../specs/preview-pipeline/README.md)
starts: which work runs before the first page, and how the rest of the
project joins the build afterwards.

### Evidence so far

- In the [Rspack pipeline lab](../rspack-pipeline/README.md#stage-2-report)
  (stage 2, two run sets, Intel macOS, headless Chrome), the first page after
  a restart with a warm cache, timed from the server reporting its URL, took:
  - 1.2 s on 5 pages, 3.3–3.4 s on 40 pages, and 7.8–13.4 s on 200 pages,
    for the Rspack pipeline;
  - 1.2–1.3 s, 1.5–1.8 s and 3.2–4.7 s for the esbuild pipeline it replaced.
- The esbuild pipeline builds only the page asked for, yet its time also grew
  with the page count. Before any page builds, the worker lists the project:
  it evaluates every preview definition, each bundled for Node by esbuild.
  That cost grows with the number of definitions, whatever the compiler. This
  is a hypothesis from the numbers above, not a measurement.
- The Rspack pipeline declares every preview and docs lens as an entry and
  builds them all before it serves the first page. Rspack's hot updates do not
  follow a module that moves to another chunk, and adding an entry can move
  shared modules into a new split chunk
  ([finding 4](../rspack-pipeline/README.md#findings)), so the entry set is
  fixed while pages are open.
- With pre-warming given time to finish, the first page showed in 0.81–0.83 s
  on 40 pages. The cost is the full build, not the page.
- Rspack's lazy compilation built single entries in 0.2–0.3 s in stage 1, but
  the first request stalled, a compile error did not show, and pages stopped
  loading after a config change
  ([finding 9](../rspack-pipeline/README.md#findings)).

### Hypothesis

- The time to the first page has three parts that grow with the project:
  listing (evaluating every definition), restoring and checking the
  persistent cache for every entry, and building every entry. The first two
  dominate a warm start.
- A chunk layout that depends only on each module, not on which entries
  exist, keeps modules from moving when entries are added. Entries can then
  join the build in any order, starting with the page asked for, and hot
  updates keep working.
- Listing can be split from building: the page list needs each definition's
  title, states and sizes, and the first page needs only its own definition.

### Variants

| # | Variant | Supported subset and missing contracts |
| --- | --- | --- |
| 0 | **Control.** The shipping pipeline: list every definition, declare every entry, build all of them, then serve | Full |
| 1 | **Requested page first, stable chunks.** The compiler starts with the requested entry only; the rest are added in batches once it is served. Split chunks are named by package (one chunk per `node_modules` package) and by project folder, so a module's chunk never depends on the entry set | Full, if hot updates hold (checked) |
| 2 | **Lazy compilation.** Every entry is declared, and Rspack builds an entry when a document first asks for it. Stage 1's problems are fixed in the prototype: GET requests forwarded to the middleware, compile errors reported for lazy entries, and the client re-created after a config change | Full, if the fixes hold |
| 3 | **A first-page compiler.** A separate one-entry compiler serves the first page while the full build runs. When the full build finishes, the page moves to it with one reload | Full; the page reloads once after start |
| 4 | **Listing split from building.** Variant 1, with the page list read from a cached index of definitions, refreshed in the background, and the first page's definition evaluated alone | Full; a stale cached list is refreshed after start |

Diagnostic controls, not candidates:

- **D1:** the control with the time split into listing, cache restore, build
  and serving, from the worker's own events. It tests the first hypothesis.
- **D2:** variant 1 with Rspack's default chunk splitting, to show whether the
  stable layout is what keeps hot updates working.

### What each prototype changes

Variants live in the lab as overlays on a snapshot of the package, as in the
Rspack pipeline lab. They change the compile worker and the pipeline module;
the canvas, runtime and preview host are the shipping ones.

### Fixtures

The [shared Acme fixture](../fixtures/README.md), with one size added:

| Size | Shape |
| --- | --- |
| `small` | 5 component previews |
| `large-system` | 40 pages |
| `huge` | 200 pages |
| `giant` | 600 pages, the `huge` shape with more pages; new for this lab |
| `formats` | Every native format, for correctness |

### Sequences

| ID | Action | Cache state |
| --- | --- | --- |
| S1 | Start the server and open one page at once | Cold: empty persistent cache, new browser profile |
| S2 | The same | Hot: the cache from S1, the same browser profile |
| S3 | Start the server, wait 2 s, open one page | Hot |
| S4 | After S2's page shows, open a second page at once | Hot |
| S5 | After S2's page shows, walk five more pages, paced | Hot |
| S6 | The [Rspack pipeline lab](../rspack-pipeline/README.md#sequences)'s E2 and E9: a component edit, and an edit to a module every page shares, on the first page and on a page opened later | Hot |
| S7 | Open the first page, then edit `workbench.config.ts` | Hot |

Each S1 and S2 sample starts its own server. The first page is a different
page in each sample, from a seeded list, so no page is favoured by the
cache.

### Boundaries

- **First page:** from the server process starting, and from the server
  reporting its URL, to the page visible, as in the Rspack pipeline lab's A1
  and A3.
- **Full build:** from the server starting to every entry built, from the
  worker's events.
- D1's parts come from worker events added in the lab: listing finished,
  compiler created, each build's start and end, and the first request served.

### Correctness checks

Every check of the [methodology](../../specs/labs.md#correctness-checks), on
every run, plus:

- **Hot updates after entries join:** with the first page open, wait until the
  rest of the project has joined the build, then make the S6 edits. Each must
  show without a reload, with React state kept. This is stage 1's finding 4
  scenario, and the reason the entry set is fixed today.
- **No moved modules:** for variants 1, 2 and 4, record each module's chunk at
  the first build and at the full build; any module that moved is a failure.
- **A compile error in a page not yet built** shows in that page's frame when
  it opens, within 1 s of its build.
- The page list matches the control's on every fixture, including after a
  definition is added while the server runs.

### Metrics

- **Primary:** S2 first page from the server's URL, on every fixture.
- **Secondary:** S1; S3; S4 and S5 selection to visible; S6 edit to visible;
  S7; full build time; worker peak memory; persistent cache size.

### Runs

Three warmups and ten samples per cell, variants in a seeded order per
fixture. Two run sets at least an hour apart, under the lab lock, with the
load recorded per sample. Each run set is smoked first with one sample per
cell on every fixture, as [runner reliability](../../specs/labs.md#runner-reliability)
requires.

### Decision rule

- A variant that fails a correctness check is rejected.
- **Flat:** a variant passes if its S2 p50 on `giant` is within 25% of its S2
  p50 on `small`, in both run sets. The control fails this by the evidence so
  far.
- **Fast:** among variants that pass, one that meets the proposed 1.5 s
  warm-cache target and the 5 s cold target on `large-system` and `giant` is
  preferred. Proposed targets stay proposed.
- **No regressions:** S4 to S6 may not be more than 10% slower than the
  control on any fixture, the owner's threshold.
- Between passing variants, the one with fewer moving parts wins unless
  another is at least 10% faster in S2 on `giant`. A reload after start
  (variant 3) counts against it.
- If listing dominates (D1), variant 4's change applies whichever compiler
  strategy wins.
- Differences within the spread of the two run sets are inconclusive.

### Host and tools

macOS 15 on Intel (16 threads, 64 GB), headless Chrome through
the standalone server, with the Rspack and loader versions the package pins.
Apple silicon is not measured. The winning change is confirmed through the
shipping server and the VS Code webview after implementation.
