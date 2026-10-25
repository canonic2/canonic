# Readiness

Part of the [preview pipeline](README.md). Readiness is the signal that a
preview has rendered enough to show and to capture. The canvas promotes a
loading frame when it is ready, and capture and export wait for it
([previews](../pages/previews.md#readiness-and-errors),
[capture](../review/capture.md)). This spec defines what it waits for, so that the
wait covers only real work.

**Status:** Implemented: the font and visible-image waits, per-phase
diagnostics, hidden-frame rendering and promotion on the renderer's ready
event. A faster font wait is an [open question](#open-questions), not an agreed
requirement; this topic records no implementation gap.

## Requirements

Readiness belongs to the requested [content renderer](../system/rendering-flow.md), not
the Workbench application document. Loading a wrapper, hiding shell chrome or
receiving metadata alone cannot declare an artboard ready. Instance, generation
and destination must match; a late report cannot reveal another request's pixels.

### What readiness waits for

After mount and the `play` and `ready` hooks, readiness waits for:

- **Fonts the page uses**, including downloads its text starts after mount.
  Readiness waits on `document.fonts.ready`.
  Checking `document.fonts.status` once is not enough: the status reads
  `loaded` before layout starts the page's own font downloads
  ([verification](#verification)). A faster rule is [open](#open-questions) and
  gets its own lab.
- **Visible images**, only those not yet complete. Offscreen lazy images are
  excluded: they keep native lazy loading and do not delay the frame swap. The
  wait is capped at 3 seconds.

It adds no fixed delay. A capture still shows the final fonts and images.
The selected page becomes visible after readiness. Fonts, CSS, images and docs
examples remain readiness blockers; Docs syntax colouring is outside readiness
([Docs host](docs-host.md#syntax-highlighting)).

Readiness must progress when the browser suspends animation frames in a hidden
loading frame: runtime readiness cannot depend on animation frames being
delivered to a hidden frame ([hidden frames](#hidden-frames)).

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
([performance](../system/performance.md#diagnostics)).

### Hidden frames

Content frames that are loading or retained stay paintable behind the visible
surface rather than being made non-renderable. This does not permit an outgoing
view beneath new destination labels, nor a nested Workbench shell. The browser
keeps painting them, so animation frames,
font loading and image decoding progress as they would on screen. Only the
active frame receives input; the others stay inert, as
[retained sessions](warm-runtime.md#retained-sessions) require.

### Promotion

After a managed document loads, the content-frame owner listens for its
`workbench:ready` event and rechecks the readiness flag before scheduling
promotion. It removes the listener and pending poll on completion, replacement,
suspension or disposal. A 50 ms poll remains a compatibility fallback for
renderers that expose only the flag; signalling docs do not wait for that poll.
After 300 unsuccessful checks the destination fails rather than becoming
visible unready ([content-frames.js](../../ui/legacy/content-frames.js),
[browser.js](../../ui/preview/browser.js)).

## Verification

[Preview runtime tests](../../ui/preview/preview-runtime.test.cjs) cover font/image waits,
errors and hidden-frame progress. The [web-font fixture](../../research/rspack-pipeline.md#stage-2-2026-10-13)
is the correctness boundary for any faster font rule: an independently observed
font download must be complete before readiness. A status-only check before
layout cannot establish that the page has started its own font loads.

## Open questions

- **Waiting for fonts the page starts.** Candidates are waiting one rendering
  frame and then on `document.fonts.ready`, or loading the families the
  page's computed styles use with `document.fonts.load`. Either needs the
  lab's readiness check to pass on the web-font fixture.
