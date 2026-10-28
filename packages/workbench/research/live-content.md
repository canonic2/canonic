# Live content isolation

## Question

When the canvas's live artboards load real previews through the previews
stream, how much do they cost the canvas, and does serving them from a
different site than the canvas let it hold more live artboards? Specifically:

- Do live artboards drop canvas frames while they load and while they receive
  hot updates, and does a separate content site or an origin per artboard
  remove that?
- How do memory, renderer processes and stream traffic grow with the number of
  live artboards (5, 10, 20)?
- Does the previews stream, which is designed for one connection per canvas,
  fit content documents that each open their own connection?

## Status

**Recommendation (2026-10-27), for the owner to decide:**

1. **Serve live content from one content site, separate from the canvas.**
   While heavy pages load, same-origin content froze the canvas for over
   100 ms 9–28 times per run (worst 494 ms); from a separate site, 0–2 times
   (worst 192 ms, usually under 80 ms). Frames over 50 ms fell by about half at
   10–20 artboards and by 90% at 5.
2. **Keeping more artboards live is cheap; making them live is not.** With 20
   live artboards, hot updates to every page cost the canvas at most one frame
   over 50 ms. Each heavy page that loads costs the canvas about one frame of
   50–80 ms even from a separate site, whether pages load all at once or two
   at a time. A higher live limit is affordable in frames if pages stay loaded
   (a larger warm pool) rather than reloading as the camera moves; its memory
   cost was not measured reliably.
3. **Do not give each artboard its own origin.** `Origin-Agent-Cluster`
   origins dropped as many frames as one site, loaded 2–3× slower (8–10 s
   for 20) and fetched more over HTTP.
4. **Change how the stream sends files to content connections.** Each
   connection receives every built file once, about 9.9 MB for this fixture,
   but the page loads its scripts through the browser's HTTP cache and never
   uses them. Twenty cross-site artboards received 197 MB; one shared
   same-origin connection 17–24 MB.

| Claim | Evidence | Result |
| --- | --- | --- |
| A separate content site removes canvas freezes over 50 ms at 5 artboards | Churn, 5 | Mostly met: 12–13 → 1 |
| A separate content site removes freezes over 100 ms | Churn and stagger, 5–20 | Met: 9–28 → 0–2 per run |
| Hot updates of live pages keep the canvas smooth | Hot, 5–20, all modes | Met: 0–1 frame over 50 ms (one same-origin run at 20: 3) |
| Loading two at a time avoids load jank | Stagger, 10 and 20 | Not met: the same cost per loaded page, spread out |
| An origin per artboard improves on one site | Churn and hot | Not met: no fewer dropped frames, slower loads |
| The stream fits per-document content connections | All cross-site runs | Met with two changes: accept the content origin; do not resend unused files per connection |
| Memory grows acceptably with live artboards | Process samples | Inconclusive: the sampling method failed twice |

Effort: about 1.5 hours of the 2.5-hour budget: one smoke run, the main
runs, one correction and one confirmation.

## Hypothesis

The [multicore record](multicore-canvas.md) showed a same-origin iframe's
script time blocks the canvas and a cross-site one does not. Real previews
also decode images, load fonts and rasterize, some of which happens outside
the content's main thread, so cross-site content should remove most but not
all load jank. Hot updates re-render each page and should behave like loading
on a smaller scale. One content site puts every live artboard in one process,
so they slow each other but not the canvas; an origin per artboard
(`Origin-Agent-Cluster`) should give one process each, at a memory cost. Each
cross-site connection receives every built file once, so stream traffic grows
with the number of connections.

## Method

All code lives in an ignored lab folder: a front door in front of the release
Workbench server, a lab content host, a canvas page and a headless Chrome
runner.

**Fixture:** the shared generator's `large-web` size at seed 1: 40 React pages
(plus 2 broken ones, excluded) sharing an app shell with web fonts, CJK text,
an icon font and a 2.5 MB PNG per page.

**Server:** the release `workbench-server` started through the host on the
fixture. Lab front doors forward every request and the previews stream to it:
one for the canvas origin (`127.0.0.1`), one content site (`localhost` on
another port) and twenty content origins (`localhost` on further ports,
answering with `Origin-Agent-Cluster: ?1`). Forwarding rewrites `Origin` to the
server's own, standing in for a server that accepts its content origins on the
stream; the product refuses other sites today.

**Content host:** loads one preview through the stream. Like the hot client,
the highest same-origin ancestor owns the connection, so same-origin
artboards share the canvas's connection and cross-site ones open their own. It
requests `previews` for its file, imports the answer's module from a blob,
mounts it, waits for the preview's readiness flag and two frames, and reports
the load time and stream bytes to the canvas. The hot client in each page
applies hot updates; the host reports the time from each `built` event to the
first DOM change.

**Canvas:** the multicore lab's main-thread JavaScript update and WebGPU
instanced renderer, with 100,000 artboards along its camera path for 7 s at
1600 × 1000 CSS pixels and device pixel ratio 2. Live artboards are
1280 × 800 iframes scaled to a quarter, in a grid above the canvas, moved with
a transform every frame.

**Variants:** `same` (content from the canvas origin), `site` (one content
site) and `origin` (one content origin per artboard), with 5, 10 and 20 live
artboards and two workloads:

- **churn:** N pages start loading 0.5 s into the camera path and N other
  pages replace them at 3.5 s, as when the live set changes while panning.
- **hot:** N pages are loaded before the path starts; the shared app shell is
  edited 1.5 s and 4.5 s in, so every live page gets a hot update.

Before the runs, every page is loaded once in each mode so the build and each
mode's HTTP cache are warm. Each variant runs on a fresh canvas document, in
rotated order, two rounds.

**Measured:** canvas frame gaps (over 20, 50 and 100 ms, p95, max) during the
path; time to ready per artboard; hot-update time; renderer processes and
resident memory of Chrome's processes compared with the same canvas before
loading; stream bytes per connection and forwarded HTTP bytes.

**Decision rule** (proposed, for the owner to confirm):

- Serve live content cross-site if it removes frames over 50 ms that
  same-origin content causes at 5 live artboards.
- Raise the live limit to N if, at N, the canvas keeps frames over 50 ms at
  zero during hot updates and no worse than at 5 during loading, and memory
  stays within about 2 GB more than the canvas alone.
- Use an origin per artboard only if it measurably improves on one content
  site.
- Change how the stream sends files to content connections if repeated files
  are a material share of load time or memory.

**Budget:** about 2.5 hours including corrections; one targeted correction,
then a decision.

**Correction before the measured runs:** the smoke run (one round, 5
artboards) warmed only the `same` mode, sampled processes without a baseline,
and waited for loads that the second churn wave had replaced. The runner now
warms every mode, samples processes on the canvas before loading, and waits
for the second wave only. Smoke results are kept but not used.

**Targeted correction and confirmation (after the main runs, before their
analysis was final):** the baseline sample still counted the previous
document's processes while they exited, so memory deltas were unreliable (some
negative). Memory is now the same canvas live minus 3 s after its artboards are
removed. One workload is added because churn loads every artboard at once:
**stagger** loads the same 2N pages two at a time (the next starts when one
reports), with the camera path repeating until all have loaded. It runs for
`same` and `site` at 10 and 20 artboards, two rounds, with `hot` repeated for
memory. The main-run frame and stream results stand.

## Conditions

macOS on a 16-thread x86_64 host shared with other sessions; load averages
were 11–19 during the main runs and 10–35 during the confirmation. Headless
Chrome (display rate 60 Hz), release `workbench-server`, Node 24. Warming
loaded all 40 pages in 11–19 s per mode with no failures.

## Results

### Main runs (2026-10-27)

Two rounds per cell; canvas frames over 50 ms (over 100 ms in brackets) and
the worst frame, during a 7 s camera path.

| Churn: N load at 0.5 s, N others at 3.5 s | 5 live | 10 live | 20 live |
| --- | --- | --- | --- |
| Same origin | 12–13 (9–11), max 267 ms | 27–31 (12–17), max 252 ms | 40–46 (27–28), max 494 ms |
| One content site | 1 (0), max 78 ms | 10–12 (1), max 192 ms | 21–23 (0–2), max 180 ms |
| Origin per artboard | 6–10 (0), max 78 ms | 18–21 (0), max 78 ms | 20–27 (0–3), max 106 ms |

| Hot: two shell edits with N live | 5 live | 10 live | 20 live |
| --- | --- | --- | --- |
| Same origin | 0, max 19 ms | 0, max 31 ms | 0, max 22 ms |
| One content site | 0, max 40 ms | 0, max 18 ms | 0–1, max 65 ms |
| Origin per artboard | 0, max 29 ms | 0, max 31 ms | 0, max 19 ms |

Hot updates reached every page: p50 4–10 ms and p95 5–21 ms from the build
event to the page's DOM change, in every mode and size.

Time to ready per artboard, p50 (all loaded together): same origin 0.7–1.1 s at
5 and 2–3 s at 20; one site 1.0–1.5 s at 5 and 2.6–3.9 s at 20; origin per
artboard 1.7–3.7 s at 5 and 6.6–9.6 s at 20. HTTP fetched through the front
doors: one site 2–9 MB per run, origin per artboard 17–107 MB, so per-origin
loads miss the HTTP cache that one site shares.

Stream traffic: one connection per cross-site artboard, each receiving
9.9 MB of built files (98.6 MB for 10 connections, 197 MB for 20, up to
389 MB when 40 loads replaced each other). Same-origin artboards shared the
canvas's one connection: 11.5–23.7 MB per run.

### Confirmation (2026-10-27)

Stagger: 2N loads two at a time. Frames over 50 ms (over 100 ms), worst frame,
and time to ready p50.

| Stagger | 10 live (20 loads, 7 s) | 20 live (40 loads, 10–11 s) |
| --- | --- | --- |
| Same origin | 25–29 (6–11), max 233 ms, ready 0.39–0.40 s | 53–58 (7–22), max 201 ms, ready 0.39 s |
| One content site | 12–18 (0), max 79 ms, ready 0.42–0.45 s | 26–28 (0), max 78 ms, ready 0.43–0.46 s |

Per loaded page this is about the churn cost: roughly 1.3–1.5 frames over
50 ms per same-origin load and 0.6–0.9 per cross-site load. Two at a time
makes each load faster (0.4 s instead of 2–4 s) but does not reduce canvas
jank. Hot repeats matched the main runs except one same-origin run at 20 (3
frames over 50 ms, max 156 ms; the hot-update p95 in that run was 161 ms).

Memory: live minus the same canvas 3 s after teardown was 0–23 MB in all but
two runs, so Chrome had not released the artboards' memory within 3 s. Neither
this nor the first method's baseline measures what live artboards hold.

## Correctness

Every one of the 40 pages loaded in every mode during warming, and no measured
run reported a failed preview. Every hot-update run delivered two updates to
every live page. Churn ready counts below 2N are first-wave loads replaced by
the second wave before they finished, as designed. Same-origin artboards
shared one stream connection and cross-site ones opened their own, as the
hot client's sharing rule predicts.

## Threats to validity

- Headless Chrome at 60 Hz on one contended x86_64 host. The Electron capture
  runtime and the VS Code webview were not measured, and whether a separate
  content site works there is open.
- The lab front doors are Node processes that forwarded every byte, including
  up to 390 MB of unused stream files per cross-site run. That CPU is on the
  same host and may account for part of the cross-site load jank; the
  remaining per-load cost was not traced, so its cause (GPU process, raster,
  image decode or host contention) is unknown.
- Origin rewriting in the front door stands in for a server change; the
  product stream refuses other sites today.
- The content host is a lab stand-in for the product renderer: it imports the
  stream's module from a blob and lets the browser fetch scripts over HTTP.
- One fixture of heavy pages (2.5 MB image, web fonts, CJK). Lighter pages will
  cost less per load.
- Memory was not measured (see Results).

## Decision and follow-ups

Recommended to the owner as stated under Status. Follow-ups if adopted:

- Serve live content from one content site and let the previews stream accept
  that origin; move same-origin conveniences (input forwarding, capture,
  annotations) to messages through the content renderer.
- Stop resending unused files on content connections: a request that omits
  files when the content document loads them over HTTP, or one connection per
  content site shared by its documents.
- Measure memory per live artboard with Chrome's own per-process memory
  reporting before raising the live limit or the warm pool.
- Trace one cross-site load to find where the remaining 50–80 ms canvas frame
  comes from, with the front doors removed.
- Check a separate content site inside VS Code webviews and the Electron
  capture runtime.
