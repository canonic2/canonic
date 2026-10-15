# First page lab

Methodology: [Workbench labs](../../specs/labs.md). Owning contracts:
[compiler](../../specs/preview-pipeline/compiler.md) for the entry set and
start, [warm runtime](../../specs/preview-pipeline/warm-runtime.md) for
pre-warming, [hot updates](../../specs/preview-pipeline/hot-updates.md) for
what must keep working, and [performance](../../specs/preview-pipeline/performance.md#measured-interactions)
for the targets.

Status: the owner agreed the plan on 2026-10-13. Run sets 1 to 3 finished by
2026-10-15 and are [reported](#report): no variant passes the decision rule.
The owner decided on 2026-10-15 to adopt none
([decisions](../../specs/preview-pipeline/README.md#decisions)).

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

Amendment, 2026-10-14, after run set 1 and before run set 2, agreed by the
owner: two combined variants join, built from what set 1 showed.

- **Variant 5** is the control with variant 4's cached index. It shows
  whether listing is the only part of the first page that grows with the
  project once the faster listing is in place.
- **Variant 6** is variant 2 (lazy compilation) with variant 4's cached
  index. In set 1, variant 2 was the fastest after the first page, and variant
  4 the fastest to it.
- Both keep variant 4's index cache inside the variant's own cache
  directory, so no variant reads another's listing, and S1 starts with none.
- Variants 5 and 6 have no set 1 samples. **Run set 3**, at least an hour
  after set 2, runs the control and variants 5 and 6 with the same sequences,
  fixtures, samples and seeded page order. For variants 5 and 6, "both run sets" in the
  decision rule means sets 2 and 3; for the others it means sets 1 and 2,
  with set 2 deciding as the previous amendment says.

Amendment, 2026-10-14, after run set 2 and before run set 3, agreed by the
owner: variants 5 and 6 measured a prototype defect in run set 2. Their
cached index skips evaluating definitions, and each entry they then prepared
evaluated its own definition in its own esbuild build, the per-definition cost
the listing change removed. On `giant`, variant 6 finished listing 0.4 s after
the worker started and began its first build at 15.9 s; variant 2, the same
compiler without the index, began at 2.3 s.

- From run set 3, variants 5 and 6 evaluate every definition the index
  skipped in one build before preparing entries, as listing does.
- Run set 2's samples of variants 5 and 6 stay in the record as measurements
  of the defect, not of the variants.
- Variants 5 and 6 are judged on run set 3 alone, and the report marks their
  result as resting on one run set.

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
| 5 | **Cached index, single build** (amendment of 2026-10-14). The control's compiler, with variant 4's cached index | Full; a stale cached list is refreshed after start |
| 6 | **Cached index, lazy compilation** (amendment of 2026-10-14). Variant 2, with variant 4's cached index | Full, if variant 2's fixes hold; a stale cached list is refreshed after start |

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
- **No moved modules:** for variants 1, 2, 4 and 6, record each module's chunk at
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

## Report

Three run sets, under the lab lock, each cell with 3 warmups and 10 samples,
at Workbench commit `89feaa6` plus this lab's uncommitted files (hashed in
each report under `measured`), on the host above:

| Set | Time (UTC) | Variants | Load average | Evidence |
| --- | --- | --- | --- | --- |
| 1 | 2026-10-13 20:34 – 2026-10-14 06:24 | 0–4, D2 | 18.4 at start, 1.9–194.8 per sample; 10 samples started above 8 | [report](results/2026-10-13-set-1/report.json), [summary](results/2026-10-13-set-1/summary.md) |
| 2 | 2026-10-14 17:08 – 2026-10-15 00:31 | 0–6, D2 | 5.5 at start, 2.1–22.7 per sample; 1 started above 8 | [report](results/2026-10-14-set-2/report.json), [summary](results/2026-10-14-set-2/summary.md) |
| 3 | 2026-10-15 02:22 – 06:59 | 0, 5, 6 | 8.7 at start, 4.3–30.2 per sample; 2 started above 8 | [report](results/2026-10-15-set-3/report.json), [summary](results/2026-10-15-set-3/summary.md) |

Set 1 lists each definition in its own esbuild build; sets 2 and 3 list them
in one. Set 2's variants 5 and 6 carry the prototype defect the amendment
before set 3 describes. Values are p50 / p95 in seconds unless marked,
recomputed from the raw samples by nearest rank.

### First page, warm cache (S2, from the server's URL)

| Variant | Set | small | large-system | huge | giant | giant ÷ small |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 0 | 1 | 1.2 / 1.7 | 3.8 / 5.5 | 9.4 / 9.8 | 24.2 / 25.8 | 20.1 |
| 0 | 2 | 1.1 / 1.2 | 2.8 / 3.5 | 7.3 / 8.0 | 14.1 / 16.2 | 12.5 |
| 0 | 3 | 1.3 / 1.7 | 3.3 / 4.1 | 6.9 / 11.3 | 15.9 / 18.4 | 12.2 |
| 1 | 1 | 1.4 / 2.5 | 2.8 / 3.2 | 5.8 / 6.1 | 14.1 / 29.4 | 10.4 |
| 1 | 2 | 1.2 / 1.3 | 2.6 / 2.7 | 3.5 / 5.9 | 4.9 / 5.8 | 4.0 |
| 2 | 1 | 1.2 / 1.6 | 3.1 / 3.7 | 6.1 / 6.6 | 13.9 / 15.4 | 11.3 |
| 2 | 2 | 1.2 / 1.3 | 2.2 / 2.4 | 2.9 / 3.2 | 4.0 / 4.2 | 3.3 |
| 3 | 1 | 1.1 / 1.5 | 3.8 / 4.0 | 7.3 / 8.4 | 14.0 / 16.2 | 12.8 |
| 3 | 2 | 1.1 / 1.2 | 3.1 / 3.6 | 4.4 / 5.8 | 7.1 / 7.3 | 6.3 |
| 4 | 1 | 1.3 / 1.5 | 2.6 / 2.9 | 3.0 / 3.5 | 4.0 / 7.2 | 3.1 |
| 4 | 2 | 1.3 / 1.3 | 2.5 / 3.1 | 2.9 / 5.5 | 3.8 / 4.5 | 3.1 |
| 5 | 2, defect | 1.1 / 1.3 | 3.7 / 5.3 | 11.7 / 12.7 | 28.1 / 31.7 | 25.1 |
| 5 | 3 | 1.2 / 1.6 | 3.6 / 5.0 | 7.1 / 9.4 | 14.9 / 16.4 | 12.4 |
| 6 | 2, defect | 1.2 / 1.3 | 3.1 / 3.1 | 8.7 / 10.5 | 17.6 / 21.1 | 14.5 |
| 6 | 3 | 1.3 / 1.7 | 2.6 / 3.4 | 3.1 / 3.9 | 4.4 / 4.8 | 3.3 |
| D2 | 1 | 1.5 / 1.6 | 2.9 / 3.2 | 6.2 / 7.0 | 14.3 / 15.1 | 9.3 |
| D2 | 2 | 1.2 / 1.4 | 2.5 / 2.8 | 3.5 / 4.4 | 4.9 / 5.7 | 4.1 |

S3 counts its 2 s wait. On `small` it adds 1.5–1.9 s to S2 for every
variant. On `giant` it is within 0.4 s of S2 for variants 0, 2, 5 and 6, and
2.3 s faster for variant 3 in set 2; for variants 1, 4 and D2 it is 1.5–4.8 s
slower, because the batches that start during the wait delay the page.

### First page, cold cache (S1)

| Variant | small, set 1 / 2 / 3 | giant, set 1 / 2 / 3 |
| --- | --- | --- |
| 0 | 4.0 / 3.9 / 4.2 | 23.3 / 13.1 / 15.1 |
| 1 | 4.4 / 3.9 / — | 17.1 / 6.1 / — |
| 2 | 4.0 / 3.9 / — | 16.5 / 6.7 / — |
| 3 | 3.8 / 4.0 / — | 19.2 / 9.6 / — |
| 4 | 3.8 / 3.9 / — | 15.8 / 6.0 / — |
| 5 | — / 3.8 / 4.0 | — / 13.4 / 14.7 |
| 6 | — / 4.0 / 4.3 | — / 6.4 / 7.2 |
| D2 | 4.4 / 3.9 / — | 15.5 / 5.8 / — |

p50 only. Set 3's variant 5 on `giant` has 9 samples: one ended on a browser
timeout.

### Where the start goes on `giant` (S2, worker events)

Seconds from the compile worker starting, p50:

| Variant | Set | Listed | First build starts | Page served | Full build |
| --- | --- | ---: | ---: | ---: | ---: |
| 0 | 1 | 11.1 | 15.0 | 23.4 | 23.3 |
| 0 | 2 | 1.0 | 4.9 | 13.2 | 13.1 |
| 0 | 3 | 1.2 | 5.4 | 15.0 | 14.9 |
| 1 | 2 | 1.0 | 3.1 | 4.0 | 82.9 |
| 2 | 2 | 1.1 | 2.3 | 2.8 | 2.8 |
| 3 | 2 | 1.0 | 2.3 | — | 10.6 |
| 4 | 2 | 0.4 | 2.8 | 2.8 | 94.6 |
| 5 | 3 | 0.4 | 5.0 | 14.1 | 14.0 |
| 6 | 3 | 0.4 | 2.4 | 2.9 | 2.9 |
| D2 | 2 | 1.0 | 3.3 | 4.1 | 39.3 |

Variant 3 serves its first page from its own compiler, which records no serve
event. On `small`, every variant served its page 0.8–1.0 s after the worker
started.

### After the first page

On `giant`. S4 is p50 / p95 over 10 samples; S5 the p50 of five paced pages;
S6 the range of its four edits, one sample each; S7 one sample.

| Variant | Set | S4 second page | S5 paced | S6 edits | S7 config edit |
| --- | --- | ---: | ---: | ---: | ---: |
| 0 | 1 | 197 / 241 ms | 143 ms | 832–917 ms | 10.8 s |
| 0 | 2 | 204 / 247 ms | 168 ms | 835–914 ms | 11.4 s |
| 0 | 3 | 210 / 298 ms | 175 ms | 932–1015 ms | 13.2 s |
| 1 | 1 | 1500 / 4222 ms | 180 ms | 3916–4136 ms | 94.7 s |
| 1 | 2 | 1703 / 2239 ms | 158 ms | 3683–3734 ms | 78.9 s |
| 2 | 1 | 391 / 456 ms | 382 ms | 219–266 ms | 2.4 s |
| 2 | 2 | 411 / 472 ms | 391 ms | 230–238 ms | 2.6 s |
| 3 | 1 | 466 / 537 ms | 160 ms | 864–1385 ms | 11.7 s |
| 3 | 2 | 502 / 2741 ms | 189 ms | 842–1284 ms | 12.2 s |
| 4 | 1 | 897 / 1280 ms | 169 ms | 3447–3633 ms | 76.8 s |
| 4 | 2 | 1085 / 1451 ms | 182 ms | 3888–3999 ms | 82.7 s |
| 5 | 3 | 236 / 345 ms | 158 ms | 855–951 ms | 11.8 s |
| 6 | 2 | 391 / 462 ms | 407 ms | 238–265 ms | 2.8 s |
| 6 | 3 | 443 / 622 ms | 567 ms | 1333–2501 ms | 3.0 s |
| D2 | 1 | 1623 / 1989 ms | 156 ms | 864–897 ms | 32.7 s |
| D2 | 2 | 1723 / 2205 ms | 169 ms | 884–933 ms | 33.3 s |

On `large-system` the pattern is the same at smaller sizes: S4 p50 was
176–195 ms for the control, 283–296 ms for variants 2 and 6, 432–528 ms for
variant 3 and 555–1176 ms for variants 1 and 4; S5 was 143–151 ms for the
control (456 ms in set 1) and 273–558 ms for variants 2 and 6. Variant 6's
`large-system` config edit in set 3 took 61.6 s, one sample; in set 2 it took
1.5 s, and variant 2 1.5–2.3 s.

The worker's peak memory on `giant`, one value per session, was 1.2–1.4 GiB
for variants 2 and 6, against 6.2–7.7 GiB for every other variant.

### Correctness

| Check | 0 | 1 | 2 | 3 | 4 | 5 | 6 | D2 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Hot updates after entries join (S6), state kept | pass | pass | pass | pass | pass | pass | pass | fail in set 1 (shared module, first page, `large-system`) |
| No moved modules | pass | pass | pass | pass | pass | pass | pass | pass |
| Compile error shown in a page not yet built | pass | pass | pass | pass | pass | pass | pass | pass |
| Config edit: the page comes back | pass | pass | pass | pass | pass | pass | pass | pass |
| Every format and docs lens renders | pass | pass | pass | pass | pass | pass | pass | pass |
| Full build completes | pass | pass | pass | pass | pass | pass | pass | pass |

Sessions run on `large-system`, `giant` and `formats`. No run wrote to a
fixture's project. D2's `large-system` session in set 2 ended on a browser timeout before its
checks, a runner failure; its `giant` session passed them. The page list was
the same for every variant on every fixture. The check for a definition added
while the server runs was not made by the runner.

### Analysis

- **Listing was the largest growing cost.** In set 1, the control listed 600
  definitions in 11.1 s of its 23.4 s; with one esbuild build, 1.0–1.2 s.
  Every variant's `giant` first page fell by 7–10 s between sets 1 and 2,
  except variant 4, which skipped listing in both.
- **The control still grows with the project** after listing: it prepares
  every entry (about 4 s on `giant` before the first build starts) and builds
  every entry (about 8–10 s) before it serves a page. Its `giant` first page
  stays 12× its `small` one.
- **Stable chunks keep hot updates working when entries join** (variants 1
  and 4 pass every hot-update and moved-module check; D2, with Rspack's
  default chunks, failed one in set 1). That supports the hypothesis. But
  joining in batches costs every later build: on `giant` the full build took
  83–95 s, edits 3.4–4.1 s and a config edit 77–95 s, against 13 s, 0.9 s and
  11–13 s for the control.
- **Lazy compilation (variants 2 and 6) held every check in every set**, so
  stage 1's problems are fixed in the prototype. It is the fastest after the
  first page on `giant`: full build 2.8–2.9 s, edits 0.22–0.27 s (sets 1 and
  2) and config edits 2.4–3.0 s, with a fifth of the memory. Switching pages
  is slower: S4 about twice the control and S5 2–3 times, since a page's
  first visit builds it.
- **The cached index removes listing but saves little** once listing is one
  build. Variant 6's first build began at 2.4 s, against 2.3 s for variant 2:
  the definitions it skips are still evaluated before entries are prepared.
  Its `giant` first page (4.4 s, set 3) is within the spread of variant 2's
  (4.0 s, set 2), measured in different sets.
- **What still grows** for variants 2 and 6 on `giant`, against `small`: about
  2 s in the worker (listing or evaluating 600 definitions, and preparing and
  declaring 600 entries) and about 0.8 s more outside the worker's own work (1.2 s from the
  server's URL against 0.4 s on `small`), which includes the canvas loading
  the page list.
  This split is read from the worker's events and the first-page times, not
  measured directly.

Threats to validity:

- Load was high: up to 195 in set 1 and up to 30 in set 3. Each start sample
  waits for a load below 8, but a session (S5–S7) waits only before it
  begins. Variant 6's `giant` session in set 3 ran at loads of 17–30, against
  7–10 for the control and variant 5; its S5 and S6 values there are not
  comparable, and set 2's (at lower load) are the reference.
- S6 and S7 are one sample per edit; their differences are exploratory.
  Variant 6's 61.6 s config edit on `large-system` is unexplained. Lazy
  compilation stalled after a config edit in stage 1, so it needs repeating
  before lazy compilation is relied on.
- Variants 5 and 6 rest on one valid run set (set 3).
- Synthetic Acme fixtures, headless Chrome, one Intel host; Apple silicon and
  the VS Code webview are not measured.

### Result against the decision rule

- **Correctness:** every candidate passes. D2, a diagnostic control, failed
  as expected.
- **Flat:** no variant passes in any set. The closest are variant 4 (3.1×
  in sets 1 and 2) and variants 2 and 6 (3.3×, sets 2 and 3).
- **Fast:** no variant meets the proposed 1.5 s warm target on
  `large-system` or `giant`.
- **No regressions:** variants 1 and 4 fail S4 and S6 on both fixtures;
  variants 2 and 6 fail S4 and S5; variant 3 fails S4 and, in one sample, an S6 edit; D2 fails S4.
  Variant 5 is within 10% of the control in set 2 and 12% slower in S4 on
  `giant` in set 3.

**No variant is adopted.** None of the starts tested shows the first page in
about its own build time on 600 pages. The evidence points at two costs no
variant removed: preparing an entry for every page before the first build,
and the canvas loading the whole page list before any page. Lazy compilation
is the strongest base for removing them: it passes every check and is the
fastest after the first page except when switching pages. Whether to build on
it, and how to make page switching and the first page independent of the page
count, is the owner's decision and needs its own plan.
