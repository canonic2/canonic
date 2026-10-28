# Multicore WebAssembly canvas

## Question

Does the canvas get user-visible wins from moving its per-update logic off the
main thread, and how much of a win comes from workers, from WebAssembly and
from using several cores? Does rendering with WebGPU in a worker keep frames
smooth while the main thread is busy, as it is when live iframes load?

## Status

**Recommendation (2026-10-27), for the owner to decide:**

1. **Serve live content from a different site than the canvas.** Chromium then
   runs it in another process, so its scripts and layout never block the
   canvas. A live iframe that janked 40 ms every 250 ms dropped 21–23 frames
   over 50 ms in 6 s when same-origin, and none when cross-site or sandboxed.
   This is the largest win measured, and it costs nothing per frame.
2. **Keep rendering on the main thread.** A WebGPU render worker only half
   hides same-origin jank (gaps p95 33 ms instead of 52 ms) and adds a frame of
   input latency (camera age p95 19 ms instead of 4 ms). With isolated live
   content it buys nothing.
3. **Multicore WebAssembly works and is about 6× faster, but is not needed
   yet.** At 100,000 artboards with 50,000 or more on screen, the update takes
   3.1 / 3.8 ms (p50 / p95) in main-thread JavaScript and 0.5 / 0.7 ms on eight
   WebAssembly threads, with bit-identical output. JavaScript already holds
   60 frames per second along the whole camera path, so the 3 ms saved is under
   the 4 ms rule and removes no dropped frame. Reopen for 120 Hz displays or
   heavier per-update logic.

| Claim | Evidence | Result |
| --- | --- | --- |
| Isolated live content prevents canvas jank | Phase B, cross-site and sandboxed iframes | Met: 0 frames over 20 ms |
| A render worker prevents canvas jank | Phase B, render worker | Partly: no frame over 50 ms, but 29–32 over 20 ms and +15 ms input latency |
| Multicore WebAssembly speeds up the update | Phase A | Met: 6× at fit-all, steady tails |
| The speedup is user-visible today at 60 Hz | Phase B, no jank | Not met: JavaScript drops no frames |
| Shared-memory WebAssembly builds without new installs | Lab build | Met: the server's pinned nightly with `rust-src` |

Effort: about one hour of the 2.5-hour budget, including one added variant
(cross-site iframe) and no corrections.

## Hypothesis

At 100,000 artboards, the swapper's update (visible set, level choice,
instance data, render-queue choice) costs several milliseconds of main-thread
JavaScript per camera change at fit-all and little at close zoom. Shared-memory
workers remove it from the main thread; WebAssembly is 1.5–2× faster per core;
several cores divide the remaining time, minus synchronization. A render worker
keeps presenting frames while the main thread is blocked, but input that
arrives on the main thread still waits.

## Method

All code lives in an ignored lab folder: a static server sending the
cross-origin isolation headers that shared memory requires, a bench page, its
workers, a dependency-free Rust crate built for shared-memory WebAssembly with
the server's pinned nightly toolchain (`-Z build-std`, atomics and bulk memory),
and a headless Chrome runner.

**Workload, one update** (the swapper's work after a camera change, for free
placement): query a uniform grid index for artboards intersecting the viewport;
choose the wanted resolution level from the zoom and each artboard's best
cached level; write 12 floats of instance data per visible artboard; choose the
64 most urgent uncached artboards by distance from the centre. Cached levels
and atlas coordinates are seeded so about half of the artboards have images.

**Layouts:** deterministic, seed 1; 1,000 and 100,000 artboards of mixed sizes
(phones, tablets, desktops, components) in clusters.

**Engines** (identical output required):

| Engine | Where the update runs |
| --- | --- |
| JS main | The main thread, as today (control) |
| JS 1 worker | One worker on shared memory |
| JS N workers | N workers, each one band of grid rows |
| Wasm 1 | One worker, Rust compiled to WebAssembly |
| Wasm N threads | N workers sharing one WebAssembly memory, one band each |

N is 4 and 8. Partitioned engines write one instance segment and one
candidate list per band; the renderer draws segments as separate ranges and
the 64 candidates are merged from the bands.

**Phase A, compute:** a scripted camera path (fit-all to 1:1 and back, with
pans) of 240 updates per run, alternating engine order, three runs per engine.
Measured from the main thread: camera written to shared memory and workers
woken, to all bands done (`Atomics.waitAsync`), so wake-up and
synchronization are included. Reported by visible-count bands.

**Phase B, pipeline:** 100,000 artboards drawn with one instanced draw per
segment. Variant 1: logic and WebGPU on the main thread. Variant 2: logic in
WebAssembly threads and WebGPU in a worker with an OffscreenCanvas. The main
thread drives the camera at display rate and is blocked for 40 ms every
250 ms, like a loading live iframe. Measured: presented frame intervals where
rendering happens, and the age of the camera each presented frame shows.

**Decision rule** (proposed by the agent, for the owner to confirm): multicore
WebAssembly is worth its build step and threading if, at 100,000 artboards, it
removes frames over 16.7 ms that the best JavaScript variant still drops, or
cuts p95 update time by at least 4 ms in a common camera state. A render
worker is worth it if it removes dropped frames under main-thread jank.

**Budget:** about 2.5 hours including corrections; one targeted correction,
then a decision.

**Amendment (before the Phase B runs):** the first Phase B smoke run showed a
sandboxed live iframe did not block the canvas. A cross-site variant (the
canvas on `127.0.0.1`, the iframe on `localhost`, both served with
cross-origin isolation and resource-policy headers) was added, since it keeps
same-origin semantics inside the content. Each jank iframe posts a message per
burst so a missing load cannot pass for missing jank.

## Conditions

macOS 15 on a 16-thread x86_64 host shared with other sessions. Load averages:
14–39 during Phase A, 6–15 during Phase B. Headless Chrome (display rate
60 Hz), device pixel ratio 2, 1600 × 1000 CSS pixels. WebAssembly built with
the pinned nightly toolchain, `opt-level = 3`, LTO, atomics and bulk memory, no
SIMD.

## Results

### Phase A: update time (2026-10-27)

100,000 artboards, three rounds of 240 updates per engine, rotating order;
milliseconds from waking the workers to every band done, measured on the main
thread.

| Engine | ≥ 50,000 visible, p50 / p95 / max | 5,000–50,000, p50 / p95 | < 5,000, p50 / p95 |
| --- | --- | --- | --- |
| JS, main thread | 3.10 / 3.76 / 4.17 | 0.59 / 1.64 | 0.005 / 0.15 |
| JS, 1 worker | 3.10 / 5.92 / 9.09 | 0.69 / 1.72 | 0.045 / 0.21 |
| JS, 4 workers | 1.08 / 2.71 / 8.04 | 0.32 / 0.71 | 0.06 / 0.15 |
| JS, 8 workers | 1.16 / 4.18 / 6.14 | 0.32 / 0.75 | 0.085 / 0.18 |
| Wasm, 1 thread | 1.84 / 2.20 / 2.24 | 0.38 / 0.99 | 0.04 / 0.13 |
| Wasm, 4 threads | 0.64 / 0.95 / 1.07 | 0.20 / 0.44 | 0.055 / 0.11 |
| Wasm, 8 threads | 0.51 / 0.73 / 0.86 | 0.19 / 0.31 | 0.075 / 0.11 |

At 1,000 artboards every engine stays under 0.25 ms at p95; waking workers
(about 0.05 ms) makes the main thread fastest there. WebAssembly is 1.7× faster
per core; JavaScript workers scale on average but have 6–11 ms outliers that
WebAssembly threads do not.

### Phase B: frames under a janky live iframe (2026-10-27)

100,000 artboards, 6 s camera path at 60 Hz, two rounds; per run: frames over
20 ms and over 50 ms between newly presented cameras, and camera age at
presentation (p95).

| Variant | Over 20 ms | Over 50 ms | Gap p95 | Camera age p95 |
| --- | --- | --- | --- | --- |
| Main thread, no iframe | 0–1 | 0 | 18 ms | 4 ms |
| Worker, no iframe | 1–3 | 0 | 18 ms | 19 ms |
| Main thread, same-origin iframe | 23 | 21–23 | 51–54 ms | 4 ms |
| Worker, same-origin iframe | 29–32 | 0–5 | 33 ms | 19 ms |
| Main thread, sandboxed iframe | 0–1 | 0 | 18–19 ms | 4 ms |
| Worker, sandboxed iframe | 0–7 | 0 | 19 ms | 19 ms |
| Main thread, cross-site iframe | 0 | 0 | 17–18 ms | 4 ms |
| Worker, cross-site iframe | 1–4 | 0 | 17–19 ms | 19 ms |

The main thread itself blocked 24 times only with the same-origin iframe.

## Correctness

All seven engines produced bit-identical outputs (instance data digest,
visible count and the merged 64 render candidates) at four steps of the path
in every run, at both sizes. Every jank iframe ran: 25 bursts per 6 s run.

## Threats to validity

- Headless Chrome presents at 60 Hz; 120 Hz displays, Electron and the VS Code
  webview were not measured, and only one x86_64 host.
- The jank is a synthetic 40 ms busy loop; real live content also lays out,
  paints and loads, but on the same thread for same-origin frames.
- Phase B drew flat instances without textures; texture uploads were not
  part of the update.
- The workload is the swapper's update for free placement; cache eviction and
  decoding were not included.
- `localhost` and `127.0.0.1` were used as two sites; a product needs its own
  scheme for a separate site that also works in VS Code.

## Decision and follow-ups

Recommended to the owner as stated under Status. Follow-ups if adopted:

- Serve live content from a separate site and move every same-origin
  convenience (shell input forwarding, capture, annotation bridging) to
  messages through the content renderer, which the rendering-flow contract
  already requires.
- Check that a separate site works in VS Code webviews and with cross-origin
  isolation, which shared-memory WebAssembly would also need.
- Keep the WebAssembly kernel and build in the lab; reopen at 120 Hz or when
  per-update logic grows.
