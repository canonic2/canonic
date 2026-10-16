# Readiness

Part of the [preview pipeline](README.md). Readiness is the signal that a
preview has rendered enough to show and to capture. The canvas promotes a
loading frame when it is ready, and capture and export wait for it
([previews](../previews.md#readiness-and-errors),
[capture](../capture.md)). This spec defines what it waits for, so that the
wait covers only real work.

## Requirements

### What readiness waits for

After mount and the `play` and `ready` hooks, readiness waits for:

- **Fonts the page uses**, including downloads its text starts after mount.
  Readiness waits on `document.fonts.ready`, as the shipping runtime does.
  Checking `document.fonts.status` once is not enough: the status reads
  `loaded` before layout starts the page's own font downloads
  ([verification](#verification)). A faster rule is [open](#open-questions) and
  gets its own lab.
- **Visible images**, only those not yet complete. Offscreen lazy images are
  excluded, and the wait is capped at 3 seconds.

It adds no fixed delay. A capture still shows the final fonts and images.

### Phase diagnostics

Each readiness phase is logged through `preview.browser.phase` with the
duration of that phase, on the event that ends it:

| Event | Duration covers |
| --- | --- |
| `hooks.completed` | The `play` and `ready` hooks |
| `fonts.completed` | The font wait |
| `images.completed` | The image wait |
| `ready` | Total time from mount to ready |

A slow wait is then visible in the Workbench log without a profiler
([performance](performance.md#diagnostics)).

### Hidden frames

Frames that are loading or retained render behind the visible frame rather
than being hidden. The browser keeps painting them, so animation frames,
font loading and image decoding progress as they would on screen. Only the
active frame receives input; the others stay inert, as
[core](../core.md#canvas-and-artboard) requires.

## Status

Implemented: each phase on its own event, and loading frames rendered behind
the visible one. Readiness still waits on `document.fonts.ready` and on every
visible image until the font-wait lab settles a faster rule.

## Verification

[Preview runtime tests](../../ui/preview/preview-runtime.test.cjs) cover font/image waits,
errors and hidden-frame progress. The [web-font fixture](../../labs/rspack-pipeline/README.md#stage-2-report)
is the correctness boundary for any faster font rule: an independently observed
font download must be complete before readiness. A status-only check before
layout cannot establish that the page has started its own font loads.

## Open questions

- **Waiting for fonts the page starts.** Candidates are waiting one rendering
  frame and then on `document.fonts.ready`, or loading the families the
  page's computed styles use with `document.fonts.load`. Either needs the
  lab's readiness check to pass on the web-font fixture.

- **Showing a page before readiness.** Should a page become visible when it
  mounts, with full readiness kept for captures? Measure the visible-load
  benefit and capture fidelity separately
  ([performance](performance.md#open-questions)).
