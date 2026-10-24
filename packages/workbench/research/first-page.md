# First page after start

## Question

How can the first page after Workbench starts show in about the time that
page's own build takes, whatever the number of pages in the project, while hot
updates keep working for every open page? The answer decides how the
[preview pipeline](../specs/preview-pipeline/README.md) starts: which work runs
before the first page, and how the rest of the project joins the build.

## Status

Decided, 2026-10-15: no variant adopted
([pipeline decisions](../specs/preview-pipeline/README.md#decisions)). Run sets 1–3
ran 2026-10-13 to 2026-10-15. The follow-up is
[instant first page](instant-first-page.md).

## Hypothesis

- Time to the first page has three parts that grow with the project: listing
  (evaluating every definition), restoring and checking the persistent cache
  for every entry, and building every entry. The first two dominate a warm
  start.
- A chunk layout that depends only on each module, not on which entries
  exist, keeps modules from moving when entries are added, so entries can join
  in any order, starting with the page asked for, with hot updates working.
- Listing can be split from building: the page list needs each definition's
  title, states and sizes; the first page needs only its own definition.

Evidence before the plan: in the [Rspack pipeline](rspack-pipeline.md#stage-2-2026-10-13),
the first page after a warm restart took 1.2 s on 5 pages, 3.3–3.4 s on 40 and
7.8–13.4 s on 200 for the Rspack pipeline (1.2–1.3, 1.5–1.8 and 3.2–4.7 s for
the esbuild pipeline). The esbuild pipeline builds only the requested page,
yet still grew with page count, pointing at listing. The Rspack pipeline fixes
its entry set because modules moving between chunks break hot updates
([stage 1, finding 4](rspack-pipeline.md#stage-1-findings)); its lazy
compilation was unreliable (finding 9).

## Method

**Variants.**

| # | Variant |
| --- | --- |
| 0 | Control: list every definition, declare every entry, build all, then serve |
| 1 | Requested page first with stable chunks: the compiler starts with the requested entry; the rest join in batches once it is served; split chunks are named by package and project folder |
| 2 | Lazy compilation: every entry declared, built on first request; stage 1's problems fixed (GET forwarding, errors for lazy entries, client re-created after a config change) |
| 3 | A one-entry compiler serves the first page while the full build runs; the page moves to the full build with one reload |
| 4 | Variant 1 with the page list from a cached index refreshed in the background, and the first page's definition evaluated alone |
| 5 | Control's compiler with variant 4's cached index (added 2026-10-14) |
| 6 | Variant 2 with variant 4's cached index (added 2026-10-14) |

Diagnostic controls: D1 the control's start split by worker events (listing,
cache restore, build, serving); D2 variant 1 with Rspack's default chunk
splitting.

**Fixtures.** The [shared Acme fixture](../fixtures/labs/README.md):
`small` (5), `large-system` (40), `huge` (200), `giant` (600, new) and
`formats` for correctness.

**Sequences.** S1 start and open one page at once, cold (empty persistent
cache, new browser profile); S2 the same, hot; S3 start, wait 2 s, open;
S4 a second page at once after S2; S5 five more pages, paced; S6 a component
edit and a shared-module edit on the first page and a later page; S7 a
`workbench.config.ts` edit. Each S1/S2 sample starts its own server and opens a
different page from a seeded list.

**Boundaries.** First page from the server process starting and from the
server reporting its URL, to the page visible. Full build from server start to
every entry built, from worker events.

**Correctness checks.** Every methodology check, plus: hot updates after
entries join (S6 with state kept), no module moving between shared chunks
between the first and full build, a compile error in a not-yet-built page
shown within 1 s of its build, and the page list matching control's.

**Runs.** Three warmups and ten samples per cell, variants in a seeded order
per fixture, two sets at least an hour apart, each smoked first. Each sample
waits up to ten minutes for a one-minute load average below 8 and records its
starting load.

**Decision rule.** A variant failing a check is rejected. Flat: S2 p50 on
`giant` within 25% of S2 p50 on `small`, in both sets. Fast: among passing
variants, prefer one meeting the proposed 1.5 s warm and 5 s cold targets on
`large-system` and `giant`. No regressions: S4–S6 no more than 10% slower than
control. Fewer moving parts wins unless another is at least 10% faster on
`giant`; a reload after start counts against variant 3.

**Amendments.** 2026-10-13, before any run: samples wait for low load (the
Rspack pipeline's confirmation was spoiled by host load). After smoke runs:
warm samples share one warm cache per variant and fixture; prototype fixes
that change no boundary; the moved-module check counts shared chunks only.
2026-10-14, after set 1: the product listed definitions in one esbuild build
instead of one per definition ([page listing](page-listing.md)), and every
variant carries that change, so set 2 decides; variants 5 and 6 were added.
2026-10-14, after set 2: variants 5 and 6 measured a prototype defect (each
entry evaluated its own definition in its own build); from set 3 they evaluate
skipped definitions in one build, and are judged on set 3 alone.

## Conditions

macOS 15 on Intel (16 threads, 64 GB), headless Chrome through the standalone
server, Rspack and loaders as the package pins, at Workbench `89feaa6` plus the
lab's files (hashed in each report). Apple silicon and the VS Code webview were
not measured.

| Set | Time (UTC) | Variants | Load average |
| --- | --- | --- | --- |
| 1 | 2026-10-13 20:34 – 2026-10-14 06:24 | 0–4, D2 | 1.9–194.8 per sample; 10 started above 8 |
| 2 | 2026-10-14 17:08 – 2026-10-15 00:31 | 0–6, D2 | 2.1–22.7; 1 above 8 |
| 3 | 2026-10-15 02:22 – 06:59 | 0, 5, 6 | 4.3–30.2; 2 above 8 |

## Results

Values are p50 / p95 in seconds unless marked, recomputed from raw samples by
nearest rank.

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

S3's 2 s wait adds 1.5–1.9 s on `small` for every variant; on `giant` it is
within 0.4 s of S2 for variants 0, 2, 5 and 6, 2.3 s faster for variant 3 and
1.5–4.8 s slower for variants 1, 4 and D2, whose batches delay the page.

### First page, cold cache (S1), p50

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

### Where the start goes on `giant` (S2, worker events)

Seconds from the compile worker starting, p50:

| Variant | Set | Listed | First build starts | Page served | Full build |
| --- | --- | ---: | ---: | ---: | ---: |
| 0 | 1 | 11.1 | 15.0 | 23.4 | 23.3 |
| 0 | 2 | 1.0 | 4.9 | 13.2 | 13.1 |
| 1 | 2 | 1.0 | 3.1 | 4.0 | 82.9 |
| 2 | 2 | 1.1 | 2.3 | 2.8 | 2.8 |
| 3 | 2 | 1.0 | 2.3 | — | 10.6 |
| 4 | 2 | 0.4 | 2.8 | 2.8 | 94.6 |
| 5 | 3 | 0.4 | 5.0 | 14.1 | 14.0 |
| 6 | 3 | 0.4 | 2.4 | 2.9 | 2.9 |
| D2 | 2 | 1.0 | 3.3 | 4.1 | 39.3 |

### After the first page, `giant`

S4 p50 / p95 over 10 samples; S5 p50 of five paced pages; S6 range of four
edits, one sample each; S7 one sample.

| Variant | Set | S4 second page | S5 paced | S6 edits | S7 config edit |
| --- | --- | ---: | ---: | ---: | ---: |
| 0 | 2 | 204 / 247 ms | 168 ms | 835–914 ms | 11.4 s |
| 0 | 3 | 210 / 298 ms | 175 ms | 932–1015 ms | 13.2 s |
| 1 | 2 | 1703 / 2239 ms | 158 ms | 3683–3734 ms | 78.9 s |
| 2 | 2 | 411 / 472 ms | 391 ms | 230–238 ms | 2.6 s |
| 3 | 2 | 502 / 2741 ms | 189 ms | 842–1284 ms | 12.2 s |
| 4 | 2 | 1085 / 1451 ms | 182 ms | 3888–3999 ms | 82.7 s |
| 5 | 3 | 236 / 345 ms | 158 ms | 855–951 ms | 11.8 s |
| 6 | 2 | 391 / 462 ms | 407 ms | 238–265 ms | 2.8 s |
| 6 | 3 | 443 / 622 ms | 567 ms | 1333–2501 ms | 3.0 s |
| D2 | 2 | 1723 / 2205 ms | 169 ms | 884–933 ms | 33.3 s |

On `large-system` the pattern is the same at smaller sizes. Worker peak memory
on `giant` was 1.2–1.4 GiB for variants 2 and 6 against 6.2–7.7 GiB for every
other variant. Variant 6's `large-system` config edit in set 3 took 61.6 s (one
sample, unexplained; 1.5 s in set 2).

## Correctness

Every candidate passed every check in every set: hot updates after entries
join with state kept, no moved modules, compile errors shown in pages not yet
built, the page back after a config edit, every format and docs lens rendering
and the full build completing. D2 failed one hot-update check in set 1 (shared
module, first page, `large-system`), as a diagnostic control may. No run wrote
to a fixture. The page list matched on every fixture; the check for a
definition added while running was not made by the runner.

## Threats to validity

High load (up to 195 in set 1, 30 in set 3); sessions S5–S7 wait for low load
only before they begin, so variant 6's set 3 session values are not comparable
and set 2 is the reference. S6 and S7 are one sample per edit. Variants 5 and 6
rest on one valid set. Synthetic Acme fixtures, headless Chrome, one Intel
host.

## Decision and follow-ups

- Listing was the largest growing cost: control listed 600 definitions in
  11.1 s of its 23.4 s in set 1, and 1.0–1.2 s once listing used one build.
- Control still grows after listing: it prepares (about 4 s) and builds
  (about 8–10 s) every entry before serving a page on `giant`.
- Stable chunks keep hot updates working when entries join (variants 1 and 4
  pass; D2 failed once), but joining in batches costs every later build
  (full build 83–95 s, edits 3.4–4.1 s, config edits 77–95 s on `giant`).
- Lazy compilation (variants 2 and 6) held every check and is the fastest
  after the first page except when switching pages, with a fifth of the
  memory.
- The cached index removes listing but saves little once listing is one build.
- Against the rule: no variant is flat (closest 3.1–3.3×), none meets the
  proposed warm target, and every candidate except variant 5 regresses S4, S5
  or S6.
- No variant adopted (owner, 2026-10-15). Two costs remained: preparing an
  entry for every page before the first build, and the canvas loading the
  whole page list before any page. [Instant first page](instant-first-page.md)
  took this up.
