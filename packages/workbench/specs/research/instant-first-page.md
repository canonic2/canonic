# Instant first page and Rust preview host

## Question

Can a Rust preview host, using Rspack's Rust crates directly, show the first
page and the complete page list within the agreed instant targets whether a
project has 5, 600 or 10,000 pages, by starting from that page alone and a
saved page index, while edits, config edits and page switches stay within the
owner's 10% rule? The answer decides how Workbench's preview host is built and
how it starts, and what of the pipeline Rust can carry.

Targets agreed by the owner on 2026-10-15, for Workbench definitions with a
warm cache, at any page count: the first page visible within 500 ms of the
canvas opening; the complete page list visible within 200 ms, on first paint;
with a cold cache, the first page in about its own build time.

## Status

Stage 1 reported 2026-10-15: every capability passed and the Rust host was
faster in every build cell, but two proposed stage 2 gates were missed. The
owner decided the same day to implement the Rust page index as measured
([pipeline decisions](../preview-pipeline/README.md#decisions)) and continue
with stage 2 in parallel. Stage 2 reported 2026-10-16: R is the leading
variant, much faster than control from 40 pages up, but no variant meets the
agreed targets or the rule; adoption is the owner's decision.

## Hypothesis

- A Rust page index (walk, check against a saved index, parse changed
  definitions for their list fields) answers in well under 200 ms at 10,000
  pages warm and in about a second cold.
- A Rust host that starts with the requested page as its only entry and adds
  an entry when a page is first opened builds the first page in about its own
  build time at any page count, with hot updates and Fast Refresh, and adds a
  page in under 300 ms.
- Without the Node binding, each build and rebuild is at least 10% faster than
  the same work through `@rspack/core`.

Evidence before the plan: [first page](first-page.md) showed every growing
cost came from the start's design. Research on 2026-10-15 (code and
documentation, no builds): Rspack publishes its Rust crates with each release
(`@rspack/core` 2.2.7 is crate version 0.102.7), unstable and to be pinned
exactly; the crates expose a compiler builder, `build()`, `rebuild(changed,
deleted)`, replaceable file systems and native HMR, lazy compilation, split
chunks, SWC, React Refresh, CSS, persistent cache and watcher crates. The Vue
loader, `css-loader`, `style-loader`, the Astro compiler, project Rspack
plugins and definition/config evaluation stay in JavaScript. The canvas waits
for `/_workbench/config` before routing, which awaits an index evaluating every
definition.

## Method

### Stage 1, parts without the canvas

Capabilities, pass or fail on `small` and `formats`: C1 the crates at 0.102.7
build on a pinned toolchain; C2 a React entry builds from virtual sources with
SWC, React Refresh and native CSS into memory; C3 an entry joins a running
compiler without rebuilding others; C4 an edit emits a hot update the shipping
hot client can apply; C5 the persistent cache restores across a restart; C6
the stable chunk layout keeps shared modules from moving when entries join; C7
a compile error is reported for its entry alone; C8 the watcher reports source
and `workbench.config.ts` changes.

Measurements: I1 cold page index (Rust against the shipping index); I2 warm
index, nothing changed; I3 warm with 1% of definitions retitled; B1 one page's
build in a new one-entry compiler, empty cache; B2 the same with a warm
persistent cache; B3 adding one entry to a compiler holding 1, 20 and 100; B4
a component edit and a shared-module edit with 1, 20 and 100 entries. B cells
compare the Rust host with a Node script running `@rspack/core` 2.2.7 with the
same options, so they differ only in the host.

Stage 1 gate (proposed): C1–C4 pass, I2 p50 ≤ 50 ms on `colossal`, B3 p50 ≤
300 ms with 100 entries. The move counts as a speed gain if Rust is at least
10% faster in B1, B2, B3 or B4.

### Stage 2, end to end through the shipping canvas

The Workbench server, canvas, preview host, runtime and hot client are the
shipping ones in every variant; what varies is everything behind
`/_workbench/previews/`.

| # | Variant | Supported subset |
| --- | --- | --- |
| 0 | Control: the shipping server, worker and pipeline | Full |
| R | Page-only start in Rust: the Rust host replaces the preview worker, serves the preview routes, starts with no page and adds a page as an entry on first request; entry sources generated in Rust from definitions parsed as literals, others evaluated by a Node subprocess in one batch; no pre-warming | React and plain-module previews; Vue, HTML, Astro, React Native Web, docs lenses and project Rspack plugins missing |
| RN | R adding up to 8 neighbouring pages after a page shows and the host is idle for 500 ms | As R |

Diagnostic controls: N, control with `server.js` on the public port itself, for
the server's front-door comparison; D1, per-request server and host timing as
the baseline for moving the Workbench server to Rust; D2, an entry cap of 20
(answered without a run set, see amendments).

Sequences: S1 cold start (no saved index, empty compiler cache, new profile);
S2 warm start; S2i warm with 1% of definitions retitled while stopped; S3
start, wait 2 s, open; S4 a page not yet opened; S5 five unopened pages, 1 s
apart; S5r returns to five opened pages in new frames; S6 component and
shared-module edits; S7 config edit; S8 add, rename and retitle a definition
while running; S9 open 100 pages, then repeat S4 and S6.

Boundaries: first page and page list from the canvas document's navigation
start (primary), also from the reported URL and from server start; the page
list counted against the fixture's manifest; own build time from a one-entry
Rust host beside S1.

Correctness: every methodology check on the subset, plus page-list parity at
every point, hot updates after pages join and after S9, no moved modules,
errors shown only in their own frame, the page back after config edits, and
the address's page, state and size.

Decision rule: a variant failing a check is rejected. Instant (agreed): S2 p50
within 500 ms for the first page and 200 ms for the page list on every
fixture, both sets. Own build time (proposed): S1 p50 at most 25% over the
page's own build. Switching (proposed): S4 and S5 p50 ≤ 300 ms on the three
larger fixtures. No regressions: S3, S5r, S6, S7 and every `small` cell within
10% of control. Long sessions: S9's S4 and S6 within 10% of the same variant's.
Fewer moving parts wins unless another is 10% faster in S4 or S5.

### Fixtures and runs

The [shared Acme fixture](../../fixtures/labs/README.md), seed 1: `small` (5),
`large-system` (40), `giant` (600), `colossal` (10,000 pages of 2–4 sections;
one definition in 40 takes its title and another its states from a module, so
5% need evaluation) and `formats`. Stage 1: three warmups and ten samples per
cell, one set. Stage 2: three warmups and ten samples per start cell, five per
session sequence, two sets at least an hour apart. Every sample waits up to ten
minutes for a one-minute load average below 8.

### Amendments

Before stage 1's run set (2026-10-15): `colossal` uses 2–4 sections per page
(about 300 MB in 30,071 folders); C1 failed on stable 1.99.0 (a nightly-only
feature in Rspack's crates), so the host builds with Rspack's
`nightly-2026-04-16` and its lockfile from Rspack's `v2.2.7`; the Rust index
has a `walk` warm check (read every directory) and a `dirs` check (stat known
directories, read only changed ones); the Node comparison host mirrors the Rust
host's options; the Rust subset is React and plain-module adapters without
project plugins; runner and prototype smoke defects were fixed.

Before stage 2's run sets (2026-10-16): the package was restructured (Node
server and worker under `server/node/`, adapters under `ui/preview/`, the page
index as the `server/` Rust package) and the lab follows it; the shipping
server starts the preview worker on the first preview request, so the runner
warms every variant with a descriptor request; on `colossal` control's first
request fails after the server's 120 s deadline while every page builds, so
the runner retries for up to 15 minutes and a start sample that ends on the
page's error is recorded as an error; prototype fixes that change no boundary
(cached config evaluation, bounded neighbours, the Node fallback running the
bundled runtime as Node, 120 s edit timeouts). D2 was left out: removing a page
from the compiler disposes its modules in every document, and returning to it
fails with a disposed-module error unless the runtime reloads it.

## Conditions

The first page lab's host (macOS 15 on Intel, 16 threads, 64 GB). Rspack
crates 0.102.7 with `nightly-2026-04-16` and a pinned `Cargo.lock`;
`@rspack/core` 2.2.7 for control; Node 24; headless Chrome for stage 2. The
VS Code webview and Apple silicon were not measured.

- Stage 1: 2026-10-15 22:03–22:29 UTC, load 5.4–10.1, none started above 8;
  measured at public `6d9d0b5` plus the lab's files (hashed in the report).
- Stage 2: set 1 2026-10-16 06:49–12:43 UTC (none above 8); set 2 13:44–22:49
  UTC (23 samples above 8 after the wait, mostly control's `colossal` cells,
  under other builds). The stage 2 runner did not record the measured source;
  the lab's files did not change between the sets.

## Results

Values are p50 / p95 in milliseconds by nearest rank unless marked; p95 over
ten samples is exploratory.

### Stage 1, 2026-10-15

Capabilities: C1 pass with the nightly toolchain (fails on stable 1.99.0); C2
to C8 pass. No shared module moved in C6 or any of the 208 B3 samples. On both
hosts only the plain-module adapter of the formats fixture builds; its React
pages need project plugins and Vue/HTML need JavaScript loaders, as the subset
lists.

Page index (`shipping` is `Compiler.index()` in a new Node process; process
start adds 12–25 ms for Rust):

| Fixture | I1 cold, shipping | I1 cold, Rust | I2 warm, `walk` | I2 warm, `dirs` | I3 1% retitled, `dirs` |
| --- | ---: | ---: | ---: | ---: | ---: |
| `small` (5) | 161 / 180 | 2.5 / 2.7 | 2.5 / 2.9 | 1.6 / 1.9 | 2.3 / 2.5 |
| `large-system` (40) | 229 / 234 | 6.1 / 6.8 | 5.1 / 5.7 | 2.6 / 2.8 | 3.9 / 4.2 |
| `giant` (600) | 913 / 959 | 38 / 39 | 30 / 32 | 8.5 / 9.0 | 14 / 14 |
| `colossal` (10,000) | 12,859 / 20,656 | 1,067 / 1,110 | 364 / 379 | 108 / 123 | 214 / 263 |

The Rust index matched the shipping index in all 208 comparisons and listed
every retitled page in all 52 I3 samples. On `colossal`, cold Rust indexing is
about half evaluation of the 500 computed definitions; warm `dirs` is about
45 ms loading the saved index, 50 ms checking directories and 13 ms checking
definition files.

Builds (`rust` against `node` with the same options):

| Fixture | B1 rust | B1 node | B2 rust | B2 node |
| --- | ---: | ---: | ---: | ---: |
| `small` | 63 / 71 | 82 / 87 | 20 / 43 | 47 / 94 |
| `large-system` | 244 / 253 | 312 / 392 | 195 / 221 | 432 / 447 |
| `giant` | 243 / 262 | 319 / 426 | 192 / 205 | 432 / 440 |
| `colossal` | 241 / 250 | 315 / 325 | 188 / 197 | 432 / 449 |

From process spawn, B1 took 259–262 ms for Rust and 516–521 ms for Node on the
three larger fixtures.

| Fixture | Held | B3 rust | B3 node | B4 component rust | B4 component node | B4 shared rust | B4 shared node |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `large-system` | 1 | 75 | 118 | 68 | 121 | 69 | 127 |
| `large-system` | 20 | 120 | 231 | 114 | 222 | 122 | 255 |
| `giant` | 1 | 81 | 141 | 78 | 176 | 77 | 180 |
| `giant` | 20 | 127 | 323 | 127 | 329 | 127 | 333 |
| `giant` | 100 | 322 | 909 | 317 | 1,074 | 304 | 897 |
| `colossal` | 1 | 63 | 96 | 64 | 116 | 63 | 120 |
| `colossal` | 20 | 100 | 218 | 101 | 222 | 100 | 221 |
| `colossal` | 100 | 234 | 717 | 238 | 678 | 238 | 682 |

Findings while preparing the host: the Rust builder leaves the parser's
`import_meta` unset for a `web` target and Rspack panics on its worker threads,
so the host passes the Node binding's parser defaults; `css/module` parser
options apply only on the rule in the `CssAutoOrModule` variant; an entry
dependency created before the persistent cache restores panics Rspack, so
entries create their dependencies on the first build that includes them; when
a page joins, other entry chunks can get new content-hash names with unchanged
content; a newly built 99 MB binary took 0.8 s to start the first time (macOS
checking a new executable) and 12 ms afterwards.

### Stage 2, 2026-10-16

Values are p50, set 1 / set 2.

Start:

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

S2i and S3 follow S2 within 10% for every variant. Where R's warm first page
goes (set 2, p50):

| Fixture | Config answer | Descriptor answer | Host's build | First page |
| --- | ---: | ---: | ---: | ---: |
| `small` | 18 | 63 | 29 | 372 |
| `large-system` | 26 | 269 | 239 | 1,114 |
| `giant` | 114 | 257 | 231 | 1,278 |
| `colossal` | 793 | 828 | 481 | 4,042 |

Control's descriptor answer is its whole start: 2.4 s on `large-system`, 15 s
on `giant` and 89–107 s on `colossal`.

Session:

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

On `small`, S4 is 64 / 62 for control and 72 / 63 for R. S8's add, rename and
retitle are within 30 ms of control on `large-system` and `giant`. Peak memory
in the session (set 2): control 4.5, 15.8 and 20.6 GB on `large-system`,
`giant` and `colossal`; R 1.0, 1.9 and 1.5 GB; RN 1.0, 1.8 and 1.8 GB.

## Correctness

Stage 1: every capability passed, with C1 needing the nightly toolchain.

Stage 2: R and RN pass every check on every fixture they ran, in both sets:
page-list parity at every point, hot updates with state kept (also after S9),
isolation, error containment in the right frame, clean console, the page back
after every config edit, no moved modules, entry and hot-client parity, no
project writes. Exceptions, none a variant defect: `formats` render parity
fails for R and RN (its React pages need the missing project-plugin contract);
two Chromium tab-creation timeouts; control on `colossal` ends 3–6 of 10 start
samples per cell on the server's 120 s preview deadline in set 2, and its set 2
session stopped after an edit did not show within 120 s.

## Threats to validity

One host and synthetic Acme fixtures whose definitions are almost all
literals; the Rust host's subset (React and plain modules without project
plugins); stage 1 B cells time the compiler, not a page in a browser, with
`@rspack/core` running the Rust host's options rather than the shipping
pipeline's; load above 8 during part of stage 2 set 2; the page list counted on
the standalone server in headless Chrome, not the VS Code webview; stage 2 did
not record its measured source.

## Decision and follow-ups

Stage 1:

- One page's build does not depend on project size with either host (B1
  241–244 ms in Rust from 40 to 10,000 pages; B2 188–195 ms).
- The Rust host is faster than `@rspack/core` in every B cell: 22–24% cold,
  55–59% warm, 34–67% adding a page and 44–71% for edits.
- The Rust index is 12–64 times faster cold and answers 10,000 pages in 108 ms
  warm with `dirs`; checking directory stamps is what keeps it under 200 ms.
- Rebuilds grow with the pages a compiler holds on both hosts, since every
  rebuild re-seals the whole compilation.
- Gate: C1–C4 pass; I2 ≤ 50 ms on `colossal` not met (108 ms); B3 ≤ 300 ms with
  100 entries met on `colossal` (234 ms) but not `giant` (322 ms); speed gain
  met. The owner implemented the Rust page index and continued with stage 2.

Stage 2:

- R's warm first page is 4–13 times faster than control from `large-system`
  up, its host builds one page in 230–480 ms on every fixture, and the two
  sets agree within 10% in every R and RN start cell.
- What remains is outside the compiler: about 0.8–0.9 s after the descriptor
  answer in the frame loading and running the page; on `colossal` the config
  answer (0.8 s) and the canvas rendering 10,000 pages (about 2 s more). The
  page list is the canvas's, not the index's (about 410 ms at 600 pages in
  every variant).
- R's hot updates take 130–170 ms on every fixture; control's grow to 800 ms
  on `giant` and 24 s on `colossal`.
- A config edit costs R about 4.2 s on every fixture, twice control's on
  `large-system`.
- Opening a page is slower in R than control at 40 and 600 pages (built on
  request), faster at 10,000.
- Long sessions degrade in both: after S9, R's S4 is 28–88% slower and its S6
  76–199% slower; control's S6 grows 9–37%.
- RN is not 10% faster than R anywhere.
- Against the rule: the 500 ms first page is met only on `small`; the 200 ms
  page list on no fixture in any variant; S1 is 6–30 times the page's own
  build; switching is met on `large-system` and `giant`, not `colossal`; S7
  regresses on `large-system`; long sessions fail. No variant meets the rule;
  R leads. Adopting R, changing a target or planning a further stage is the
  owner's decision.
