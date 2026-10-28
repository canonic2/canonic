# Artboard swapper

## Question

Can a canvas of a thousand heavy artboards stay responsive at every zoom by
showing each artboard as the cheapest representation that looks right
(placeholder, cached PNG at a fitting resolution, or live iframe), with
background rendering, caching and a texture atlas, and what limits it?

## Status

**Decided 2026-10-27:** the owner adopted the artboard swapper, with its
texture atlas, as the canvas rendering model. WebGPU draws the canvas; PNG is
the image representation, and vector replay is not pursued
([paint-record record](iframe-paint-record.md)). The contract is
[canvas rendering](../specs/canvas/rendering.md). The prototype measured
here is the reference implementation; the shipping canvas has not adopted it.

| Requirement or claim | Evidence | Result |
| --- | --- | --- |
| Zoomed-out drawing does not grow with the number of artboards | Atlas comparison | 900 draw calls → 2; frame p50 2.0 → 1.0 ms |
| Idle canvas does no queue work | Scheduler counters while idle and panning | 0 runs |
| Evicted or reloaded images return without rendering | Memory and IndexedDB refill | 0.3–0.7 s and 0.5–0.6 s for 1,000 |
| Renders reach the screen in priority order | Queue phase timings | queue wait ≈ 0 after limiting requests to the pool size |
| Swapping to a live iframe is not visible | Reveal after load and two frames | warm 0 ms; cold p50 35–177 ms behind the PNG |
| A first visit renders 1,000 heavy artboards quickly | Empty-cache fill | 16–26 s; inconclusive under host contention |
| A cache that survives a restart | none | not met: the browser cache dies with a temporary profile |

## Hypothesis

Most per-frame and per-render cost comes from work the canvas does not need:
drawing or rendering artboards nobody can see, re-rendering images already
produced, and per-artboard draw calls. Rendering in a background pool,
caching every result by content, and batching small images into shared
textures leave only visible, never-seen resolutions to render.

## Method

Variants ran as canvas prototypes in Chrome, rendering images in a pool of
headless Chromium renderers. A headless runner drove each scenario
through the same steps:

1. fill a zoomed-out view of 1,000 artboards from an empty cache;
2. drop GPU textures and refill from memory;
3. zoom to 67% (several large artboards, live iframes near the centre);
4. clear all caches and pan quickly, so queued work must be cancelled;
5. zoom to 800% and pan away and back;
6. zoom out to fit, then reload the page and refill from IndexedDB.

Readouts were the scenario's own counters (shown representations, queue and
cache state, fill sources, swap latency, scheduler runs), per-batch phase
timings (queued, set content, ready, screenshot, transfer, decode, upload)
and frame timings. The atlas comparison ran the checkpoint and the atlas
scenario alternately, twice each, and compared screenshots pixel by pixel at
5%, 67% and 800%.

Content: deterministic 640 × 480 artboards, either simple pages or ten heavy
themes (layered gradients, blend modes, filters, backdrop blur, clip paths,
about twenty system font families, several scripts, emoji and generated SVG
images), generated inline so nothing is fetched.

## Conditions

macOS on 16 CPU cores, shared with other agent workloads throughout; Chrome
headless at 1600 × 1000 CSS pixels and device pixel ratio 2; four renderers.
Absolute times vary with host load; matched comparisons ran back to back.

## Results

### Queue and render pool (2026-10-27)

The first version kept eight render requests open, batched up to 64 artboards
per sheet, ran every renderer page in one browser and navigated a page per job.

| Cause found | Effect | Change |
| --- | --- | --- |
| Six HTTP connections per origin, one held by the event stream | requests waited 1.4–13 s inside the browser before the server saw them | one request per renderer; the canvas queue keeps priority and cancellation |
| 64-artboard sheets | 1–7.5 s per sheet, blocking centre artboards | sheets of at most 16 artboards and about a megapixel |
| One browser for all renderers | screenshots serialized in its GPU process: 430 ms for 320 × 240 | one headless browser per renderer: 48–63 ms |
| Navigation per job, lazy start | 280–600 ms load per single render; 6.4 s first thumbnail | persistent page with content replacement; pool started with the server |

With simple pages, the full fill went from 23.8 s to 8.7 s, the first
thumbnail from 6–12 s to 0.8 s, and a zoomed-in render from 1.2–2 s to a
median of about 120–150 ms.

### Caches (2026-10-27)

Three tiers, keyed by a hash of each artboard's document and the resolution:
GPU textures, encoded PNG sheets in memory, IndexedDB. Before them, an evicted
texture or a reload re-rendered everything (about 24 s for the heavy themes).
After: 1,000 artboards refill in 0.3–0.7 s from memory and 0.5–0.6 s from
IndexedDB within one browser profile.

### Heavy content (2026-10-27)

Compared with simple pages, the heavy themes cost a full fill of 24.5 s
(first thumbnail 5.1 s, half 17.1 s), with a first sheet of 17.8 s attributed
to fonts and emoji loading in fresh renderers. Typical 16-artboard sheets took
0.6–0.8 s; 4× renders 0.4–0.8 s; a cold swap to a live iframe 105–177 ms p50.
Cache refills did not change.

### Atlas, event-driven scheduling and renderer warm-up (2026-10-27)

| Measure (zoomed out, 1,000 heavy artboards) | Checkpoint (two runs) | Atlas (two runs) |
| --- | --- | --- |
| Draw calls | 900 | 2 |
| Frame time while idle, p50 / p95 | 2.0 / 2.2–2.4 ms | 1.0 / 1.4–1.5 ms |
| Frame time while panning, p50 / p95 | 1.9 / 2.2 ms | 1.0 / 1.3–1.4 ms |
| Scheduler runs while idle or panning | every frame | 0 |
| GPU memory for the thumbnails | 18 MiB | 64 MiB (one fixed atlas page) |
| First thumbnail | 1.4–1.8 s | 1.3 s |
| Full fill | 17.1–25.1 s | 15.9–25.9 s |
| Memory refill / IndexedDB refill | 0.35–0.73 / 0.57–0.60 s | 0.31–0.38 / 0.47–0.57 s |
| Renderer warm-up | none | 0.5–0.7 s, skipped once warm |

At 67% (13 draws) both variants measured 0.4–0.5 ms per frame. The owner
reported the atlas canvas as far more responsive in interactive use.

## Correctness

Every run filled all visible artboards, showed live iframes at the centre at
mid zoom, cancelled work on fast pans, and refilled from both cache tiers
without errors. Atlas screenshots differed from the checkpoint in under
0.004% of pixels at 5% and 67% zoom and were identical at 800%. Real input
over a live iframe (Cmd + wheel zoom, Space + drag pan, a click reaching the
content) behaved as intended.

## Threats to validity

- The host was shared and loaded; fills varied by about 50% between rounds,
  so fill-time differences between variants are not established.
- The checkpoint's runs followed the atlas variant's warm-up in the same
  server, so renderer warm-up's effect on the first sheet is inconclusive.
- One machine, one browser build, headless; no VS Code webview or Electron.
- Artboards share one size and a grid layout; free placement and other sizes
  were not measured.
- IndexedDB refills reflect one browser profile; the prototype's profile is
  temporary, so nothing persists across sessions.

## Decision and follow-ups

Adopt the artboard swapper with the texture atlas as the canvas rendering
model ([canvas rendering](../specs/canvas/rendering.md)). Keep WebGPU drawing
and PNG images; do not pursue vector replay.

Follow-ups: a cache that survives restarts, owned by the server; the shipping
canvas migration; collections far beyond 2,000 artboards with free placement,
investigated separately.
