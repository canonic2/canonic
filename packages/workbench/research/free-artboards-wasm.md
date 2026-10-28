# 100k free-form artboards and Rust/Wasm

## Question

Can a canvas prototype show 100,000 freely placed artboards (mixed sizes,
clusters, overlaps with z-order) with smooth pan and zoom at every zoom level?
And does Rust compiled to WebAssembly pay for a build step and a second
language in the CPU-heavy parts this needs, compared with plain JavaScript?

The two candidate workloads are a spatial index over 100k rectangles and CPU
rasterization of overview tiles. The decision is where, if anywhere, compiled
code would pay off; Workbench UI delivery has no build step, so a Wasm result
is a recommendation, not shipping code.

## Status

**Decided 2026-10-27: JavaScript stays for both workloads.** Wasm is 1.4–2×
faster on most index operations and 0.9–1.6× on tile rasterization, but every
JavaScript operation already meets its proposed budget by a wide margin. The
differences are microseconds per query and a few milliseconds per screen of
tiles. Frame pacing in the integrated scenario is the same with either engine.
The one visible Wasm advantage is the first index build in a fresh page (cold
JIT), about 118 ms against 25 ms, which happens once per layout.

| Proposed budget (discovery target) | JavaScript, warm p50 / p95 | Wasm, warm p50 / p95 | Outcome |
| --- | --- | --- | --- |
| Viewport query ≤ 1 ms, up to ~50k results | 0.065 / 0.23 ms | 0.045 / 0.10 ms | Met by both |
| One overview tile ≤ 4 ms | 0.47 / 1.2 ms (8 px level) | 0.40 / 1.3 ms | Met by both |
| Screen of tiles (104 tiles) ≤ 250 ms | 16–63 / 40–84 ms | 15–70 / 20–88 ms | Met by both |
| Wasm ≥ 2× faster where JS misses a budget | — | — | No JS miss, so no Wasm case |
| Smooth pan and zoom (~60 FPS), integrated | 0 frames over 20 ms in pans and zoom sweeps (GPU-only path) | same | Met after one correction |

Effort: about three hours of the agreed four-to-five-hour budget, including three
runner corrections (a detached-buffer view, a cleanup error that discarded a
result, a results-folder clash) and one scenario correction. Closed; no
further runs are scheduled.

## Hypothesis

A packed Hilbert R-tree in typed arrays builds 100k entries in tens of
milliseconds and answers small queries in microseconds in JavaScript; Wasm
might be 1.5–2× faster on the build and roughly even on small queries, where
the JS↔Wasm call dominates. Tile rasterization is a tight integer pixel loop,
the likelier Wasm win, but it matters only if JavaScript is slow enough for a
person to notice. WebGPU drawing all 100k artboards as instanced quads in one
call might make CPU overview tiles unnecessary for frame rate; tiles would then
only add quality (area-averaged, z-ordered overview pixels).

## Method

Plan agreed with the owner before the first run. All code is in an ignored lab
folder: a canvas prototype in Chrome, a dependency-free Rust
crate (`crate-type = ["cdylib"]`, raw `extern "C"` exports, no wasm-bindgen),
a bench page and two Node runners using `scripts/chrome.cjs`.

| Part | Content |
| --- | --- |
| Control | JavaScript engine: packed Hilbert R-tree (node size 16) in `Float32Array`/`Uint32Array`; moved artboards are tombstoned and kept in an overflow list until a rebuild; fully contained subtrees are copied as one contiguous range of leaf ids; tile rasterizer with exact area coverage in 1/256-pixel fixed point, composited in z (id) order, 8×8 theme mini-images, average colour under 2 px |
| Candidate | The same algorithms in Rust, built with `cargo build --release --target wasm32-unknown-unknown` (opt-level 3, LTO, one codegen unit, `panic = "abort"`, no SIMD). Inputs are copied into linear memory; query results, the move buffer and tiles are read through `Int32Array`/`Float32Array`/`Uint8Array` views on that memory |
| Diagnostic | An empty exported Wasm function against an empty JS function, 1M calls per block (JS↔Wasm crossing) |
| Fixture | Deterministic generator, seed 1, 100,000 artboards in 373 clusters on a 538,536 × 560,747 canvas: 28,378 phones, 20,798 tablets, 19,286 desktops, 31,538 components; about 4% of neighbours overlap. Small control: the same generator at 1,000 artboards |
| Index cases | Build; 64 viewports of 1600×1000 CSS px centred on seeded artboards, at four calibrated zooms giving median results of 10, 300, 5,000 and 50,000; moving 1,000 artboards; a 300-result query with moves pending; rebuild |
| Tile cases | 104 tiles (13 × 8, a 3200 × 2000 device-pixel screen) at three levels where the median artboard spans about 2, 8 and 24 px (512, 128 and 32 world units per pixel); artboards 40 tile pixels or larger are left to the GPU |
| Boundaries | Each action timed alone with `performance.now()` in a cross-origin-isolated page (5 µs resolution); queries timed as blocks of 64 and reported as block means. Copying moves into the engine's buffer is inside the move timer; copying the layout into Wasm memory is inside the build timer; Wasm compile and instantiate are timed separately |
| Cache states | Warm: one page, both engines, three discarded warmup passes, then 20 passes alternating engine order. Cold: a new headless Chrome and profile per sample, one engine each, 10 per engine, alternating |
| Correctness | Outside every timer, before and after the warm set: sorted ids equal between engines and to a brute-force scan (all queries up to the 5,000 zoom), `count` equal to `query`, identical results after moves and after rebuild, and byte-identical tiles (FNV-1a) for all 312 tiles |
| Integrated check | The scenario in headless Chrome at 1600 × 1000, DPR 2, with two background renderers: per overview mode (mini with JS, tiles with JS, tiles with Wasm), five zoom levels (fit-all ≈ 0.0014, 0.006, 0.02, 0.08, 0.5) with a 2 s pan each, then a 3 s zoom sweep from fit-all to 0.5 and back. Presented-frame gaps from `requestAnimationFrame`, scenario CPU per frame, levels of detail and cache state |
| Decision rule | Recommend Wasm for an operation only if JavaScript misses a proposed budget (query > 1 ms per frame, a tile > 4 ms, a screen of tiles > 250 ms) and Wasm meets it or is ≥ 2× faster with identical output. Otherwise JavaScript stays |

### The scenario

One persistent instance buffer (32 bytes per artboard: rectangle, thumbnail
extent and atlas slot, theme) is written once at layout; later changes write
only the affected instances. Every frame draws all 100k artboards in one
instanced draw. The vertex shader keeps sub-pixel artboards at one pixel with
their area as coverage. The fragment shader picks a flat theme colour below
6 px, an 8×8 theme mini-image from one real render of the ten themes up to
40 px, and a 256 px atlas thumbnail (512 slots, LRU) once one exists. Above
384 device pixels an artboard gets an exact-resolution texture (at most 24),
and up to three artboards nearest the centre become live iframes once 700 CSS
px wide. Optional overview tiles (256 slots) replace artboards under 40 px once
every visible tile is ready, rasterized by the selected engine within 6 ms per
frame. Thumbnails render in sheets of 16 in the background pool, nearest the
centre first, are cancelled when they leave the view plus a 25% margin, and are
kept as encoded sheets in a 128 MiB memory tier. When nothing can need a render
(the largest artboard is under 40 px on screen), the scenario runs a
count-only query.

The prototype's canvas had a fixed 5% zoom floor, which cannot show a canvas
this size; it was given an optional lower floor for this canvas.

## Conditions

macOS 15.8 on x86_64 (16-thread Intel Xeon W-2140B, 64 GiB, AMD GCN-5 GPU),
headless Chrome 156, Node 24.12, rustc 1.93.1. Wasm module 38 KB. The host was
heavily shared with other sessions: load averages of 50–460 on 16 threads
during the runs (75–150 during the decisive bench set). Both variants ran
under the same contention, with alternating order.

## Results

Warm bench, 20 alternating passes per engine (2026-10-27). Query rows are
block means of 64 viewports; tile rows are per tile (n = 2,080) or per screen
(n = 20).

| Operation | JS p50 / p95 | Wasm p50 / p95 | JS / Wasm p50 |
| --- | --- | --- | --- |
| Build 100k | 17.8 / 32.6 ms | 9.3 / 22.6 ms | 1.9 |
| Query, ~10 results | 2.4 / 7.4 µs | 1.7 / 5.3 µs | 1.4 |
| Query, ~300 results | 6.6 / 11 µs | 3.9 / 6.2 µs | 1.7 |
| Query, ~5,000 results | 27 / 130 µs | 14 / 23 µs | 2.0 |
| Query, ~50,000 results | 65 / 227 µs | 45 / 102 µs | 1.4 |
| Move 1,000 artboards | 60 / 70 µs | 55 / 85 µs | 1.1 |
| Query ~300 with 1,000 moves pending | 19 / 155 µs | 13 / 26 µs | 1.5 |
| Rebuild | 16.7 / 24.1 ms | 8.9 / 34.4 ms | 1.9 |
| Screen of tiles, 2 px level | 23.4 / 55.9 ms | 14.5 / 19.9 ms | 1.6 |
| Screen of tiles, 8 px level | 62.6 / 83.5 ms | 70.1 / 87.6 ms | 0.9 |
| Screen of tiles, 24 px level | 16.0 / 39.9 ms | 15.3 / 45.7 ms | 1.05 |
| One tile, 8 px level | 0.47 / 1.20 ms | 0.40 / 1.35 ms | 1.2 |
| Empty call (crossing) | 7.8 ns | 12.4 ns | 0.6 |
| Small control: build 1k | 0.16 ms | 0.08 ms | 2.0 |
| Small control: query in 1k | 0.1 µs | 0.1 µs | 1.1 |

Single-tile maxima reached 134 ms (JS) and 127 ms (Wasm), each once in 2,080,
with p95 under 1.5 ms. These match descheduling on the loaded host, not an
engine property.

Cold, a fresh browser per sample (n = 10 per engine; p50, with maximum):

| Operation | JS | Wasm |
| --- | --- | --- |
| Wasm fetch, compile and instantiate | — | 12.6 ms (128 ms) |
| First build | 118 ms (325 ms) | 25 ms (142 ms) |
| First query, ~50,000 results | 0.41 ms (1.3 ms) | 0.11 ms (2.1 ms) |
| First tile | 3.0 ms (8.9 ms) | 0.9 ms (56 ms) |
| First screen of tiles, 8 px | 104 ms (206 ms) | 65 ms (171 ms) |
| First rebuild | 46 ms (187 ms) | 9.3 ms (154 ms) |

Memory: JavaScript typed arrays 5.3 MB; Wasm heap 6.6 MB in 9.8 MB of linear
memory. The two engines hold the same structures; Wasm keeps an input copy.

A smoke set earlier the same day (two passes, load average 460) gave the same
picture: build 21.7 against 9.7 ms, all queries under 0.15 ms, and tile screens
16–86 ms against 14–57 ms.

Integrated scenario, confirmation run after the correction (frame gaps over
20 ms out of about 120 per pan or 180 per sweep direction; scenario CPU per
frame p95 / maximum):

| Overview, engine | Pans (5 levels) | Zoom sweep in / out | CPU p95 / max |
| --- | --- | --- | --- |
| Mini (GPU only), JS | 0 at every level | 2 (one 317 ms gap) / 0 | ≤ 2.5 / 5.6 ms |
| Tiles, JS | 0 at every level | 0 / 0 | ≤ 8.1 / 16.9 ms |
| Tiles, Wasm | 5 at the 0.02 level, else 0 | 0 / 0 | ≤ 8.6 / 18.6 ms |

At fit-all, all 100,000 artboards are visible (count-only query) and the
screen needs 216 tiles. At 0.02, 18,072 are visible. Generating the layout took
54 ms, indexing 24 ms and uploading the instance buffer 13 ms. Moving 1,000
artboards near the centre (index edit plus 1,000 instance writes) took 4.7 ms.

Before the correction (same scenario, earlier run), zoom sweeps had up to 18
long frames per direction and scenario CPU up to 134 ms. Every frame of a zoom
created and cancelled render jobs (9,748 cancellations) and built sheet HTML
that was then discarded. Index queries in those frames took about 0.2 ms. The
correction holds render scheduling and tile rasterization while the scale
changed within the last 150 ms, and resumes once the zoom settles; the
instanced draw shows mini-images meanwhile.

## Correctness

| Check | JS | Wasm |
| --- | --- | --- |
| Query ids equal between engines (256 queries, before and after the set) | Pass | Pass |
| Equal to brute force (192 queries up to ~5,000 results) | Pass | Pass |
| `count` equal to `query` length | Pass | Pass |
| After 1,000 moves, pending and rebuilt (128 queries) | Pass | Pass |
| Tile bytes identical (312 tiles, 208 non-empty) | Pass | Pass |
| Scenario: no exceptions or GPU validation errors in the confirmation run | Pass | Pass |

The first scenario run found a GPU validation error: the exact-texture pass
could write 25 rectangles into a 24-entry buffer. It was corrected before the
confirmation run.

## Threats to validity

- The host was heavily loaded and shared. Absolute latencies and tails are
  inflated and noisy, but the comparison alternates engines under the same
  contention, and every JavaScript value is far inside its budget.
- One host: x86_64 with an AMD GPU, headless Chrome only. Apple silicon,
  Electron and the VS Code webview were not measured; a webview confirmation
  is needed only if the canvas adopts these structures.
- Wasm was built without SIMD or threads. SIMD could speed tile rasterization,
  but JavaScript does not miss the tile budget, so it would not change the
  decision.
- JavaScript's build sorts a `Float64Array` with the engine's native numeric
  sort; that is the ordinary JavaScript path, not hand-written JavaScript.
- Bench tiles use synthetic theme mini-images. Raster cost depends on coverage
  and artboard counts, not colours.
- The tile cut-off differs: 40 tile pixels in the bench, 80 in the scenario
  (a tile level is reused across up to 2× zoom). The scenario's integrated
  raster timings cover its own setting.
- Live iframes reuse the reference scenario's logic. An earlier scenario run
  recorded live swaps (p50 197 ms); the confirmation run's zoom levels did not
  assert a live swap.
- The one 317 ms frame gap in the confirmation run came while scenario CPU
  stayed under 5 ms. Its cause is unknown (host, GPU process or background
  renders).

## Decision and follow-ups

Against the decision rule: **reject Rust/Wasm for the spatial index and for
overview tile rasterization at 100k.** JavaScript meets every proposed budget,
so no operation qualifies, even where Wasm is about 2× faster. That margin is
microseconds per frame for queries, 8–9 ms per rebuild, and nothing measurable
in integrated frame pacing. A build step and a second language are not
justified for these workloads at this size.

Reopen only with a specific workload where JavaScript misses a budget. The
nearest candidates are first layout of a much larger canvas (cold JS build
grows past a few hundred milliseconds), whole-pyramid precomputation instead
of on-demand tiles, or image diffing of rendered thumbnails. The last one was
not measured.

What the evidence supports for the canvas:

- Draw every artboard from one persistent instance buffer in one instanced
  call, with level of detail chosen in the shader by on-screen size; it holds
  60 FPS from fit-all to 1:1 without CPU tiles.
- Keep a packed Hilbert R-tree with tombstoned moves and periodic rebuilds in
  plain JavaScript, with a count-only query when nothing can need a render.
- Queue render and tile work only once the zoom settles, cancel work for
  artboards that leave the view, and cap requests to what the caches hold.
- CPU overview tiles are optional. They add area-averaged, z-ordered overview
  pixels but cost frame time while panning; the GPU mini-image path is
  sufficient for frame rate.

Proposed to keep (owner decision): the JavaScript path of the prototype. That
is the layout generator, the R-tree and tile engine, without the Wasm engine
switch. The Wasm crate, bench and runners stay in the lab.
