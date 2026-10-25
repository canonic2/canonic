# Docs navigation and warm pool

## Question

Does reusing one initialized docs document across pages, swapping only the
page's Markdown and examples, shorten selection to visible enough to justify
it over shortening the separate-document waterfall, with every docs contract
intact? The decision it informs: whether docs get a warm document like managed
previews, only earlier resource discovery inside separate documents, or
neither.

## Status

Decided and confirmed, 2026-10-23. The owner selected the warm pool with
immediate resume and parallel metadata fetching (`pool-rp`), accepting about
13 MB more JS heap with four docs documents. The shipping implementation passed
a three-session confirmation, a final source pass and isolated installed Acme
checks. Current requirements live in the [Docs host](../specs/preview-pipeline/docs-host.md#reusable-page-lifecycle),
its [warm Docs pool](../specs/preview-pipeline/docs-host.md#warm-docs-pool) and
[serving](../specs/preview-pipeline/serving.md#preload-selection).

## Hypothesis

A warm docs document brings a first visit close to the cost of its
page-specific work, about 30–45 ms on Acme, because it skips document
creation, the shared module graph, stylesheet parsing and font readiness.
Earlier resource discovery inside separate documents saves the bundle-answer
round trip and part of the module wait (an expected 10–20 ms) but keeps
document creation and font readiness. Retained returns are already fast and
should not change.

Exploratory evidence before the plan (a one-off probe on Acme's 13 docs pages,
headless Chrome 1600×1000, one sample per page): first visits 74–143 ms to
frame promotion (median about 90 ms), evicted returns 76–145 ms, retained
returns 8 and 20 ms. A new docs document requested about 20 resources, of
which only the Markdown page, bundle answer, page module and chunk were
page-specific; fonts were the largest readiness wait (10–69 ms) although
already in the HTTP cache. Earlier navigation observations are under
[Results](#earlier-navigation-observations).

## Method

**Variants.**

| Variant | What it does |
| --- | --- |
| 0. Control | Shipping behavior with the readiness-event correction: one document per docs page, up to three idle documents retained for 15 minutes |
| 1. Early discovery | Separate documents; the docs HTML carries the page's bundle answer inline plus `modulepreload` and stylesheet preloads in `<head>`, applied by a lab proxy |
| 2. Warm docs host | One long-lived docs document per content renderer and lens: navigation disposes mounted examples, restores the document baseline, replaces `main.wb-docs` and mounts the next page's module; shared chunks, scripts, stylesheet and fonts stay loaded |
| 3. Warm pool | Variant 2's warm documents with control's retention: a new page reuses the oldest idle docs document of its lens once three pages are idle |

Diagnostic controls: D1 control without the `document.fonts.ready` wait; D2
variant 2 without baseline restoration. Every variant, including control, went
through the same pass-through lab proxy; each session used a new browser
profile. Warm-document isolation copies the preview host's tracking of
listeners, timers, animation frames, globals, body children and root
attributes; text `<style>` elements are removed between pages and CSSOM-built
sheets that libraries hold are kept.

**Inputs.** Acme, copied into the run directory (13 docs pages of the HTML
adapter, shared stylesheet, web fonts replayed from one recording through
request interception); `docs-small` (6 pages, system fonts, 2–4 examples);
`docs-heavy` (30 pages with React and React Native Web lenses, an environment
provider, three local web-font families, one 1200×800 image per page, 8–12
examples per page with isolation sentinels). Every page carries a runner marker
with page and revision.

**Sequences.** Cold start (generated fixtures only), first visit, retained
return, evicted return, docs lens switch, design round trip, Markdown edit,
example edit, and a 50-selection long walk followed by 10 s idle.

**Boundaries.** Selection to visible (primary): from the canvas receiving the
selection to the first animation frame where the promoted frame shows the
selected page's marker at the selected revision, all expected example markers,
fonts loaded and visible images complete, checked by the runner's own probe.
Also text visible, examples mounted, fonts ready, colours settled, capture
ready, and edit to visible.

**Correctness checks**, every run: render parity against control's
screenshots (a page fails when its share of differing pixels exceeds the
largest share between control sessions); isolation; freshness; scroll and
address; error containment; readiness and capture; cleanup after the canvas
closes; clean console. A variant failing a check is rejected, not tuned.

**Runs.** Each session launches a new profile, walks every page once as a
discarded warm-up, then opens a new canvas and measures; the first session per
variant and fixture is discarded. Run 1: ten measured sessions per variant and
fixture (at least 20 samples per first-visit and return cell), 20 cold starts
per variant on generated fixtures. Stage 2: three measured sessions per variant
and fixture.

**Decision rule** (proposed, agreed 2026-10-22). Variant 2 wins if, against
control on Acme and `docs-heavy`, it cuts first-visit and evicted-return p50 by
at least 30% and 25 ms, makes no cell's p95 worse by more than 5 ms, passes
every check, and keeps its long-walk heap within control's. Variant 1 wins
instead if it reaches two thirds of variant 2's first-visit gain. Inconclusive
if the confirmation difference falls within control's run-to-run variation.
Owner decisions on 2026-10-22: one warm document per content renderer and
lens; text before examples is measured but not counted; cold start only on the
generated fixtures.

**Smoke findings, 2026-10-22**, classified before the run set: canvas documents
loaded from the server's space address bypassed the proxy (the proxy now
rewrites the server origin and sessions fail if any frame bypasses it);
variant 2 dropped a page's scroll one navigation early (corrected); a lens
switch matched the old lens's marker (the probe now matches the lens); a reused
document preloaded an already-loaded runtime chunk (the warm host leaves that
preload out); D2 leaked every sentinel, as designed; a cold sample folder could
not be removed while a process finished writing (removal now retries).

**Stage 2 plan, agreed 2026-10-23.** A profiling pass (control and variant 3,
one traced session each, attributing every navigation's time) followed by one
comparison set of control against variant 3 with switchable fixes:
`resume` (show a retained page immediately), `parallel` (fetch the bundle
answer with the page), `memory` (release earlier pages' example registrations)
and `promote` (remove the frame wait before promotion).

## Conditions

Standalone server and headless Chrome 155 on macOS x64 (16 logical CPUs,
64 GB, AC power), Node 24.12.0, the bundled server binary at the measured
public commit plus hashes of uncommitted measured files. Run 1 on 2026-10-22
and 2026-10-23: session-start one-minute load p50 16.6, maximum 295 (a virtual
machine outside the lab used about five cores). Stage 2 on 2026-10-23: load
p50 12.88, maximum 58.83. Shipping confirmation on 2026-10-23: load p50 6.53.
Installed checks: VS Code 1.142.0 / Electron 43.7.7 / Chromium 150, isolated
profile and temporary Acme copy.

## Results

### Run 1, 2026-10-22

- Warm documents (variants 2 and 3) cut first-visit p50 by about 38% (Acme
  131 → 80 ms, heavy 148 → 93 ms) and requests per page from 22–26 to 4–6.
  Neither met the rule: retained returns were slower (Acme 26 → 79 ms for
  variant 2, 61 ms for variant 3) and the long-walk heap larger
  (10.2 → 22–23 MB).
- Early discovery (variant 1) gained 4–14%, under the threshold.
- D1 showed the font wait is not on the critical path.
- Cold start did not differ between variants.

### Stage 2 profile, 2026-10-23

Traces covered Acme only (Chrome stalled tracing the heavy fixture in
control); every session recorded Chrome's per-step counters. New-page medians:

| Path | Finding |
| --- | --- |
| Examples waterfall | Control: docs page requested 25 ms after selection, scripts at 56 ms, bundle answer 57–64 ms, module 63–79 ms, examples mounted 82 ms, fonts settled 106 ms, visible about 130 ms; each server answer takes 5–15 ms |
| Fonts and CSS | A new document waits 20–24 ms for cached fonts; a warm document about 5 ms |
| Example mounting | About 0.1 ms per example, about 1 ms per page; not a cost |
| Frame presentation | Readiness to promotion 2–3 ms; promotion to visible 15–23 ms |
| Retention | Control's retained returns 26–29 ms; the warm pool's 70 ms because it asked for the page's revision first (38–57 ms). Heap over a 50-page walk: control flat at about 12.7 MB, warm pool 19 → 24 MB |
| Server | Docs page 5–14 ms, bundle answer 5–6 ms, revision 6–15 ms to first byte |
| Background work | About 10–15 ms of canvas and renderer scripting per selection, about 5 ms of highlighting worker time, no long tasks |
| Editing and interactions | Show code opens in 21–27 ms and is coloured at 60–86 ms; scrolling and zooming hold 60 fps; Markdown edits show in 60–80 ms, example edits in 140–160 ms |

### Stage 2 comparison, 2026-10-23

Three measured sessions per variant and fixture; every variant passed every
check.

- `pool-rp` / `pool-rpm`: new pages p50 126 → 78 ms on Acme and 134 → 80 ms on
  heavy; evicted returns 110 → 62 and 156 → 80 ms; retained returns equal to
  control (26 → 27 and 29 → 29 ms); lens switches equal.
- They fail only the long-walk heap (10.3 → 22.8 MB) and show edit p95
  differences of 11–29 ms from six samples each (heavy retained-return p95 38
  against 30 ms; Acme example-edit p95 199 against 170 ms).
- `memory` did not reduce the heap: the extra memory is the warm documents' own
  runtime. `promote` improved p50 slightly but widened p95 tails. `resume`
  alone fixes retained returns; `parallel` recovers the new-page time `resume`
  alone lost.

### Warm Docs pool decision, 2026-10-23

The owner adopted the warm Docs pool with immediate retained resume and
parallel page/metadata fetching (`pool-rp`), with a nonblocking background
revision check on return (show immediately, refresh only if changed, keep
scroll, ignore late results). The goal is about 40% faster browser-warm
selection to visible for new docs destinations when a pool document can be
reused, with retained returns as fast as separate-document retention. This is
not a cold-start or universal latency claim. The accepted extra heap is a
deliberate exception to the original heap-parity rule, not a universal memory
ceiling. The background revision check was not part of the measured variant.
The server's initialized-runtime preload correction shipped first as an
independent change covering previews and docs.

### Warm Docs integration, 2026-10-23

The shipping UI implements the dedicated docs pool and reusable page lifecycle.
The server serves its authored HTML host unchanged, omits initialized runtime
preloads, and supplies separate Markdown and compiled-example revision
identities, which distinguish prose-only refreshes from missed example builds
and detect a save between parallel HTML and metadata reads. The compiled
revision uses the entry's reachable module content hashes, errors and compiler
generation. A project stylesheet's smooth-scroll setting does not animate
programmatic page reset or restoration.

| Shipping run | First visit p50 (Acme / heavy) | Retained return p50 | Evicted return p50 | Settled heavy heap |
| --- | ---: | ---: | ---: | ---: |
| Three measured sessions per fixture | 63 / 79 ms | 28 / 30 ms | 62 / 65 ms | 21.8 MB, four documents |
| Final source pass, one session per fixture | 63 / 79 ms | 28 / 30 ms | — | 21.9 MB, four documents |

Every check and cleanup assertion passed in both runs. Heavy edit p95 values
(396 ms Markdown, 262 ms examples) have six samples each. The final pass
follows explicit application of the page-state bridge after a new start and a
Markdown swap. Standalone capture preparation reports 503 without a capture
helper, so screenshot parity is not native-capture acceptance.

The installed VSIX check in an isolated VS Code profile walks beyond pool
capacity, returns with scroll preserved, opens and colours Show code, stays at
four docs documents and applies Button's disabled state to page options and
the HTML state attribute. Earlier installed attempts exposed runner
assumptions about nested canvases, foreground animation frames and Acme's
smooth scrolling.

### Earlier navigation observations

**Acme docs navigation, 2026-10-19.** Headless Chrome 1900×1100 through the
`html` docs lens: first visits to Typography/Button took 261/286 ms at public
base `3e2a86e`, and 215/170 ms with entry-script preloads, which fetched chunks
in parallel during the stylesheet wait. Fresh-server Colors startup varied from
1.4 to 2.9 s; retained returns took 17–35 ms. Single samples, before the 100 ms
loading-feedback grace period.

**Loading feedback, 2026-10-20.** The retained-session browser smoke returned
to its docs document in 22–24 ms with no frame painting the loading message;
controller tests verify the 100 ms feedback deadline and its cancellation.

**Docs readiness handoff, 2026-10-21.** An installed 0.14.1 Acme trace showed
selection to visible around 97–104 ms while the content renderer polled the
docs readiness flag every 50 ms after `workbench:ready` was already emitted. The
correction listens for the readiness event, keeping the poll as a fallback. A
probe of 12 selections per variant measured ready-to-promotion p50 27.6 ms
installed against 1.4 ms corrected, and selection to visible 146.5 against
127.1 ms.

**Pending-viewport regression, 2026-10-23.** The outgoing-view grace period
left the canvas at `display:none` on first open, so a pending content renderer
had a 0 × 0 viewport and never promoted. An isolated foreground VS Code webview
reproduced the hang. Pending renderers now get invisible, inert, correctly
sized slots independently of the displayed canvas model
([navigation](../specs/canvas/navigation.md#declared-artboard-groups)); the same
probe reached the visible phase after installation.

## Correctness

Every variant passed every check in run 1 and stage 2; D2 leaked every
sentinel, as designed. The shipping confirmation and final pass passed every
check and cleanup assertion; the installed check passed after runner
corrections.

## Threats to validity

Uncontrolled and often high host load; p95 values are exploratory, and edit
cells have six samples. Standalone headless Chrome, not the installed webview,
for timings. Shipping confirmations ran only shipping code at lower load, so
they confirm the design's scale rather than a new paired percentage gain.

## Decision and follow-ups

- Adopted: `pool-rp` with a background revision check on return; `memory` not
  required; `promote` rejected for widening tails.
- The heap exception is accepted for four docs documents only.
- Installed-webview latency is not measured; the installed check is
  compatibility evidence.
