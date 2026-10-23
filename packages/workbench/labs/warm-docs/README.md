# Warm docs lab

Status: the owner-selected warm pool with immediate resume and parallel
metadata is implemented. Shipping smoke, the three-session timing/long-walk
confirmation, the final source pass and isolated installed Acme/state checks pass.
See [adoption decision](#adoption-decision-2026-10-23) and
[shipping confirmation](#shipping-confirmation-2026-10-23). The plan and run
reports below retain their original rules and outcomes.

Method: [Workbench labs](../../specs/system/labs.md). Owning contracts:
[docs](../../specs/pages/docs-pages.md), [warm runtime](../../specs/preview-pipeline/warm-runtime.md),
[performance](../../specs/system/performance.md),
[readiness](../../specs/preview-pipeline/readiness.md),
[rendering flow](../../specs/system/rendering-flow.md) and frame retention in
[core](../../specs/core.md#canvas-and-artboard).

## Question

Does reusing one initialized docs document across pages, swapping only the
page's Markdown and examples, shorten selection to visible enough to justify
it over shortening the separate-document waterfall, with every docs contract
intact?

The decision it informs: whether docs get a warm document like managed
previews, only earlier resource discovery inside separate documents, or
neither.

## Exploratory evidence and hypothesis

Gathered before this plan with a one-off probe, not a lab result: the
repository's Acme space served by the standalone server, headless Chrome
1600×1000, tracing on, one walk of 13 Acme docs pages followed by returns,
with the uncommitted readiness-event correction in `content-frames.js`.
One sample per page; host load uncontrolled.

| Cell | Selection to frame promotion |
| --- | --- |
| First visit, 12 pages | 74–143 ms, median about 90 ms |
| Return after eviction, 4 pages | 76–145 ms |
| Retained return, 2 pages | 8 and 20 ms |

In a new docs document, `document-loaded` arrived 34–86 ms after selection,
and readiness 28–92 ms after that. Fonts were the largest single readiness
wait in the new document (10–69 ms), although every font was already in the
HTTP cache. Each new document requested about 20 resources; only the
Markdown page, the bundle answer and the page's own module and chunk were
page-specific. The docs page script and its imports, the preview runtime,
Rspack's runtime and shared chunks, the lens stylesheet, the font stylesheet
and font files were requested again for every page.

Hypothesis: a warm docs document brings a first visit close to the cost of
its page-specific work, about 30–45 ms on Acme, because it skips document
creation, the shared module graph, stylesheet parsing and font readiness.
Earlier resource discovery inside separate documents saves the
bundle-answer round trip and part of the module wait (an expected 10–20 ms),
but keeps document creation and font readiness. Retained returns are already
fast and should not change.

## Variants

| Variant | What it does | Supported subset and missing contracts |
| --- | --- | --- |
| **0. Control** | Shipping behavior with the readiness-event correction: one document per docs page, up to three idle documents retained for 15 minutes | Complete |
| **1. Early discovery** | Separate documents as in control. The docs HTML carries the page's bundle answer inline, plus `modulepreload` for the page module and a preload for the lens stylesheet in `<head>`. The page script skips its bundle request when the answer is inline | Complete. Retention unchanged |
| **2. Warm docs host** | One long-lived docs document per content renderer and docs lens. Navigation fetches the next page's HTML, disposes the mounted examples, restores the document's baseline, replaces `main.wb-docs`, then imports and mounts the next page's module. Shared chunks, the docs script, the lens stylesheet and fonts stay loaded. Retention keys stay per page; a retained page that is not the warm document's current page is restored by swapping into the warm document | Same isolation rules as the warm preview host, with the docs page's own state (scroll, Show code panels, highlighting, revision watch) reset per page. A lens switch between docs lenses creates or reuses that lens's host. Project modules are evaluated fresh per visit, as the preview host does |
| **3. Warm pool** | Added before any run (see [method details](#method-details-fixed-before-the-first-run-2026-10-22)). Variant 2's warm documents, with control's retention: a new page reuses the oldest idle docs document of its lens once three pages are idle | As variant 2 |

Variants 1 and 2 are implemented in the lab only. The runner copies the
product UI into the run's temporary directory and overlays the variant files,
as earlier labs did. Variant 1's HTML change is applied by a lab proxy in front
of the unchanged server; every variant, including control, goes through the
same pass-through proxy so its cost is equal.

## Diagnostic controls

Never adopted; they isolate costs.

- **D1. No font wait.** Control with the docs page's `document.fonts.ready`
  wait removed. Separates font readiness from document creation.
- **D2. Unisolated swap.** Variant 2 without baseline restoration between
  pages. Measures what isolation costs in a warm document.

## Inputs

All variants see identical inputs.

- **Acme demo.** The repository's first space, read-only: 13 docs pages of
  the HTML adapter, the shared `assets/acme.css` and web fonts. It already
  renders quickly in some cells, which guards against a variant that only
  helps heavy pages.
- **`docs-small`** (generated, seed 1): 6 Markdown pages, HTML adapter, system
  fonts, 2–4 examples per page.
- **`docs-heavy`** (generated, seed 1): 30 Markdown pages with a React lens
  and a React Native Web lens, an environment provider and global stylesheet,
  three local web-font families (one used only on every fourth page), one
  1200×800 image per page, 8–12 examples per page with code fences. Every
  example leaves isolation sentinels: a window listener, an interval, a portal
  in `body`, a `body` class, a window global, an injected style and a request
  mock, and records in a runner-owned marker what an earlier page left.

The measured fixtures used deterministic generation, with pinned dependencies in a lab
lockfile. Every page carries a runner marker in its Markdown and in each
example with the page and revision.

## Expected results

Decided before any run, for every page and lens: the Markdown marker for the
selected page and revision is visible; every example shows its own marker
with status `ready`, or the declared error for the fixture's one broken
example; the fonts the page uses report loaded; visible images are complete;
no sentinel from another page is present.

## Sequences and cache states

| Cell | Sequence | Cache state |
| --- | --- | --- |
| Cold start | Open the canvas on a docs page in a new profile and compiler | Cold |
| First visit | Select a page this canvas has not shown | Browser-warm |
| Retained return | Return to one of the last three pages | Retained |
| Evicted return | Return to a page shown more than four selections ago | Browser-warm |
| Docs lens switch | Switch between the heavy fixture's two docs lenses | Browser-warm |
| Design round trip | Docs, then design, then back to docs on a page with both | Browser-warm |
| Markdown edit | Write a fixture Markdown file; stop on the edited marker | Retained |
| Example edit | Write an example file; stop on the edited example marker | Retained |
| Long walk | 50 selections over the heavy fixture, then 10 s idle | Browser-warm |

Edited files are restored in `finally`, and the next sequence waits for the
original output.

## Boundaries

All timers run in the runner over CDP, against `performance.now()` in the
canvas document, with document timestamps converted through
`performance.timeOrigin`.

- **Selection to visible (primary).** Starts when the canvas receives the
  selection; stops on the first animation frame where the promoted frame
  shows the selected page's Markdown marker at the selected revision, all
  its expected example markers, its fonts loaded and its visible images
  complete, checked by the runner's own probe, not by the product's flag.
- **Text visible.** Same start; stops when the Markdown marker is painted in
  the visible frame. Variant 2 may show text before examples; whether that is
  allowed is a product decision, so the primary metric does not count it.
- **Examples mounted, fonts ready, colours settled, capture ready.** Same
  start; each stops on its own observation (example markers, `document.fonts`
  status of the page's families, `whenHighlighted()`, a successful capture
  preparation of the visible frame).
- **Edit to visible.** Starts when the runner writes the file; stops on the
  edited marker.

The existing `preview.browser.phase` trace explains each sample. The probe
adds only reads.

## Correctness checks, every run

- **Render parity:** each page and lens matches the control's screenshot at
  the same size.
- **Isolation:** no sentinel from an earlier page on the current one; no
  leftover `body` children, `html`/`body` attributes, styles, timers,
  listeners or globals beyond the document's baseline after a swap.
- **Freshness:** Markdown and example edits reach the canvas; an unchanged
  page is not stale; returning to a page evaluates its project modules fresh.
- **Scroll and address:** a new page opens at the top, a docs link to
  `#example-<id>` scrolls to it, and a retained page keeps its scroll.
- **Error containment:** the broken example shows its error and others mount.
- **Readiness and capture:** `__workbenchReady` still waits for examples,
  fonts and images; captures match the control.
- **Cleanup:** after the canvas closes, no docs document, worker or
  `EventSource` listener remains.
- **Clean console:** no errors or warnings the control does not produce.

A variant that fails a check is rejected, not tuned.

## Metrics

- Primary: selection to visible, p50 and p95 per variant and cell.
- Secondary: text visible, examples mounted, fonts ready, colours settled,
  capture ready, edit to visible; long tasks during a navigation; resources
  requested and bytes per navigation; JS heap and renderer process memory
  after the long walk and with three retained pages; renderer CPU over the
  10 s idle after the walk.

## Runs

- Two discarded warmup walks per variant and fixture.
- At least 20 samples per variant and cell. First-visit and evicted-return
  samples come from repeated walks in fresh canvas sessions on a browser-warm
  profile, since each page is unvisited only once per session.
- Variants alternate by session in a seeded order (seed 1); fixtures and
  sequences keep a fixed order within a session.
- Smoke: control and variant 2 with one sample per cell on every fixture
  before the run set. Each smoke failure is classified as runner defect or
  finding first.
- One full run set, then a confirmation set of control and the leading variant.

## Decision rule (proposed targets)

- **Variant 2 wins** if, against control on both Acme and `docs-heavy`, it
  reduces first-visit and evicted-return p50 selection to visible by at least
  30% and at least 25 ms, makes no cell's p95 worse by more than 5 ms, passes
  every correctness check, and its long-walk heap stays within control's
  (control can hold four docs documents).
- **Variant 1 wins instead** if it reaches at least two thirds of variant 2's
  first-visit gain while passing every check, since it keeps separate
  documents and needs no isolation machinery.
- **Both may be adopted** if their gains add up in a combined spot check.
- **Inconclusive** if the confirmation set's difference falls within the
  run-to-run variation of control between the two sets.

## Host and tools

This Mac (OS, architecture, CPU, memory and power recorded per run), Node and
the bundled `workbench-server` binary at the measured public-repository
commit, plus hashes of uncommitted measured files (including
`ui/legacy/content-frames.js`). Headless Chrome through
`scripts/chrome.cjs`, version recorded. A result the decision relies on is
confirmed later in the installed VS Code webview.

## Budget and authorization

- No paid or external API calls.
- Network: installing `docs-heavy`'s pinned React dependencies into the run
  directory, and the Acme demo's web-font requests to Google Fonts. Acme's font
  requests are served from the browser cache after the first visit, but cold
  cells reach the network; the owner decides whether cold Acme cells run or
  are replaced by the generated fixtures' local fonts.
- Nothing measured leaves the machine.

## Owner decisions, 2026-10-22

1. Variant 2's scope is agreed: one warm docs document per content renderer
   and docs lens, separate from the preview host.
2. The proposed decision thresholds are agreed as written.
3. Text before examples is measured but does not count toward the primary
   metric.
4. Cold start is measured on the generated fixtures only. Acme runs
   browser-warm and retained cells.

## Method details fixed before the first run, 2026-10-22

Settled while preparing the runner, before any sample:

- **Variant 3, warm pool.** Added beside variant 2. Variant 2, as agreed,
  moves its one document from page to page, so earlier pages are no longer
  retained and a retained return becomes a swap. Variant 3 keeps control's
  retention. A new page reuses the oldest idle docs document of the same lens
  once three pages are idle, the point where control would evict one, and
  otherwise opens a new document as control does. Its eviction order can
  differ from control's when the oldest idle session is not a docs document of
  that lens. Variant 3 is judged by the same rule as variant 2; if both win,
  the one with the better first-visit p50 leads the confirmation set.
- **Variant 2 scroll.** Variant 2 remembers the scroll position of the last
  three pages it showed, so the retained-scroll check applies to it as well.
- **D2** applies to variant 2.
- **Overlays.** One lab proxy per variant forwards to one unchanged server per
  fixture and replaces only the variant's files:
  `content-frames.js` and `preview-sessions.js` (variants 2, 3, D2), the docs
  page script (variant 1, D1), `mount-examples.js` (D1) and the lab's warm
  docs host (variants 2, 3, D2). Control's proxy forwards everything. The
  proxy rewrites `Origin` to the server's, since the server refuses other
  origins. Each session uses a new browser profile, so overlaid files never
  share a cache across variants.
- **Variant 1's HTML** is rewritten by its proxy: the bundle answer replaces
  the bundle request, and the head gains `modulepreload` links for the page
  module, the preview runtime and the docs scripts, and `preload` links for
  the module's chunk scripts and stylesheets. The proxy fetches the bundle
  answer and module from the server before answering, a cost a server
  implementation would not pay twice; it is counted against variant 1.
- **Warm document isolation.** Variants 2 and 3 copy the preview host's
  tracking of listeners, timers, animation frames, globals, `body` children
  and `html`/`body` attributes. Between pages they remove `<style>` elements
  written as text and keep CSSOM-built sheets that a library keeps
  references to, such as React Native Web's. Module stylesheets the next page
  does not ask for are removed after its module loads; the ones it asks for
  stay parsed, with their fonts.
- **Acme** is copied into the run directory: `assets/`, `system/` and
  `previews/`, with a `workbench.yaml` holding the Acme docs pages, the HTML
  docs lens and Button's design. Each Markdown file gains one runner marker
  after its title. Google Fonts responses are fetched once per run set and
  served to the browser from that recording through DevTools request
  interception, so measurements do not depend on the network.
- **Capture ready** is the product readiness flag in the visible document; the
  standalone runner has no capture helper. Capture parity uses screenshots.
- **Request mocks** are not among the generated fixture's isolation sentinels;
  docs examples do not declare them.
- **Samples.** Each session launches a new browser profile, walks every page
  once as its discarded browser warm-up (taking the parity screenshots), then
  opens a new canvas and measures. The first session per variant and fixture
  is discarded as process warm-up. Ten measured sessions per variant and
  fixture, with a 400 ms pause between steps. First visits, retained returns
  and evicted returns reach at least 20 samples; lens switch, design round
  trip and edits take two samples per session. Cold start takes 20 samples per
  variant and generated fixture, each with a new server, empty preview disk
  cache and new profile.
- **Parity** compares each session's warm-up screenshots with the first
  control session's. A page fails when its share of differing pixels (any
  channel off by more than 8) exceeds the largest share between control
  sessions of that page; a missing comparison counts as not checked.

## Smoke runs, 2026-10-22

Before the run set, control and the candidates ran one sample per cell on
every fixture. Each failure was classified before the run set:

| Smoke | Failure | Classification and correction |
| --- | --- | --- |
| 1 | Every wait timed out; the report was lost when the run was stopped | Runner defect: the canvas loads content renderers from the space address the server returns, which bypassed the proxy, so no overlay applied and the probe could not reach the documents. The proxy now rewrites the server's origin in JSON answers, each session fails if any frame bypasses the proxy, and sessions are saved as they finish |
| 2 | Variant 2 lost a retained page's scroll position | Variant defect: its scroll memory dropped a page one navigation early. Corrected to keep the three pages before the current one |
| 2 | Lens switch measured 1 ms in control | Runner defect: both lenses carry the page marker, so the old lens's document stopped the timer. The probe now also matches the document's lens |
| 2 | Variants 2 and 3 logged Chrome's unused-preload warning | Finding: in a reused document the shipping page module preloads the page's runtime chunk and then skips it, since the profile's runtime is loaded. The warm preview host can show the same warning. The lab's docs host leaves that preload out, as a product version would in the module |
| 3 | D2 leaked every sentinel | Expected: D2 has no isolation, which confirms that the sentinels detect leaks |
| 3 | A cold sample's folder could not be removed while a process finished writing; the run ended before parity | Runner defect: removal now retries and records a runner cleanup failure without ending the run. A missing parity comparison is reported as not checked |

Host load was high throughout: a virtual machine outside the lab used about
five cores, and the 1-minute load average reached 26–83 on 16 cores.

## Run-1 report, 2026-10-23

Full results: [results/2026-10-22-run-1](results/2026-10-22-run-1/README.md),
recomputed from raw samples by `summarize.cjs`. Ten measured sessions per
variant and fixture, 20 cold starts per variant on the generated fixtures. The
1-minute load average at session start had a p50 of 16.6 and a maximum of 295,
so p95 values are exploratory.

- **Correctness:** every variant passed every check on every run; D2 leaked
  every sentinel, as designed.
- **Warm documents (variants 2 and 3)** cut first-visit p50 by about 38%
  (Acme 131 → 80 ms, heavy 148 → 93 ms) and requests per page from 22–26 to
  4–6. Neither meets the decision rule: retained returns are slower (Acme p50
  26 → 79 ms for variant 2, 61 ms for variant 3) and the long-walk heap is
  larger (10.2 → 22–23 MB).
- **Early discovery (variant 1)** gains 4–14%, under the threshold.
- **D1** shows the font wait is not on the critical path.
- **Cold start** does not differ between variants.

Result against the decision rule: no variant adopted. Variant 3's retained
returns should match control's, since it keeps control's documents; that gap
and the heap growth are open questions for stage 2.

## Stage 2 plan, agreed 2026-10-23

The owner asked to settle the remaining investigation paths in one shorter
stage instead of one lab each. Same fixtures (Acme and heavy only), probe,
correctness checks and decision rule as above.

1. **Profiling pass, no candidates.** Control and variant 3, one measured
   session each per fixture, with a Chrome performance trace over the whole
   walk. For every navigation it attributes time to: the network waterfall
   with server response times (paths 2 and 7), main-thread work by category
   and by script (paths 4, 5 and 8, including capture, DOM mirroring and
   highlighting), mount time per example and environment hook (path 4), and
   readiness → promotion → visible with the animation frames between them
   (path 5). It also measures Show code latency, frame times while scrolling
   and zooming (path 9), and heap after each page of a 50-page walk in
   variant 3 (path 6). Not a timing comparison; its numbers choose the fixes.
2. **One comparison run set.** Control against variant 3 with corrections
   for its retained-return gap and heap growth, and against variant 3 plus any
   further fixes the profile shows, each fix switchable on its own as a
   separate variant. Three measured sessions per variant and fixture after one
   discarded session; no cold cells, since run 1 found no cold difference.

## Stage 2 profile report, 2026-10-23

Evidence: [results/2026-10-23-profile-4](results/2026-10-23-profile-4/README.md)
(one traced session per variant and fixture; `profile`, `profile-2` and
`profile-3` are failed attempts, kept as records). Chrome stalled when one
trace covered the heavy fixture in control, at the first new page after a
retained return, even with per-step traces and without the verbose timeline
category; the same session runs through untraced. Traces therefore cover
Acme only, and every session records Chrome's per-step counters instead.
Medians for new pages unless stated.

| Path | Finding |
| --- | --- |
| 2 Examples waterfall | Control: docs page requested 25 ms after selection, docs scripts run at 56 ms, bundle answer 57–64 ms, module 63–79 ms, examples mounted 82 ms, fonts settled 106 ms, visible about 130 ms. The steps are sequential but each server answer takes 5–15 ms; the warm document removes the document and script-graph steps. Fetching the bundle answer with the page is the remaining overlap (stage 2 fix `parallel`) |
| 3 Fonts and CSS | A new document waits 20–24 ms for fonts already in the HTTP cache; a warm document waits about 5 ms. D1 showed the wait is not removable: the fonts still have to load in each new document |
| 4 Example mounting | Each example mounts in about 0.1 ms and a page's examples in about 1 ms; environment hooks are negligible. Not a cost |
| 5 Frame presentation | Readiness to promotion takes 2–3 ms; promotion to visible on the canvas 15–23 ms, about one frame plus the message to the canvas (stage 2 fix `promote` removes the frame wait before promotion) |
| 6 Retention | Control's retained returns take 26–29 ms. The warm pool's took 70 ms because its host asked the server for the page's revision first (38–57 ms, mostly waiting on a busy main thread; stage 2 fix `resume`). Heap over a 50-page walk: control flat at about 12.7 MB; warm pool 19 → 24 MB, growing as each warm document keeps earlier pages' examples registered (stage 2 fix `memory`) |
| 7 Server | Docs page 5–14 ms, bundle answer 5–6 ms, revision 6–15 ms to first byte. Not the bottleneck |
| 8 Background work | About 10–15 ms of canvas and renderer scripting per selection (`renderer-bridge.js`, `content-frames.js`, `workbench.js`), one capture-preparation request in the middle of each navigation and about 5 ms of highlighting worker time. No long tasks. The main thread waits for about a third of a control navigation |
| 9 Editing and interactions | Show code opens in 21–27 ms and is coloured at 60–86 ms; reopening is immediate. Scrolling and zooming hold 60 frames per second. Markdown edits show in 60–80 ms, example edits in 140–160 ms |

Stage 2 candidates, cumulative on variant 3: `pool-r` (resume), `pool-rp`
(+ parallel), `pool-rpm` (+ memory), `pool-rpmc` (+ promote). Smoke
`smoke-5-stage2`: every check passed for control and `pool-rpmc`.

## Stage 2 report, 2026-10-23

Evidence: [results/2026-10-23-stage2](results/2026-10-23-stage2/README.md),
three measured sessions per variant and fixture. Every variant passed every
check.

- **`pool-rp` / `pool-rpm`** (warm pool with immediate resume and the bundle
  answer fetched with the page): new pages p50 126 → 78 ms on Acme and
  134 → 80 ms on the heavy fixture, evicted returns 110 → 62 and 156 → 80 ms,
  retained returns equal to control (27–29 ms), lens switches equal. Against
  the rule they fail only on the long-walk heap (10.3 → 23 MB) and on edit
  p95 differences of 11–29 ms from six samples each, within run-to-run noise.
- **`memory`** did not reduce the heap: the extra memory is the warm
  documents' own retained runtime, not earlier pages' registry entries.
- **`promote`** improves p50 slightly but widens p95 tails on both fixtures.
  Rejected.
- **`resume` alone** fixes retained returns; **`parallel`** recovers the new-page
  time `resume` alone lost.

Decision for the owner: adopt the warm pool with `resume` and `parallel`,
accepting about 13 MB more JS heap with four retained docs documents, or
reject it under the agreed heap rule.

## Adoption decision, 2026-10-23

The owner selected the warm pool with `resume` and `parallel` (`pool-rp`),
accepting about 13 MB extra JS heap with four Docs documents (one active,
three idle). The original rule and its reported failures above remain the lab
record. The `memory` change did not reduce the cost and is not required;
`promote` is rejected because it widened tails.

The product additionally requires a nonblocking background revision check on
retained resume: show the page immediately, refresh only if changed, retain
scroll, and ignore late results after navigation or disposal. The measured
immediate-resume variant did not include that check. Ship the independent
initialized-runtime preload correction first, since it also benefits previews.

Current requirements replace the lab variant contracts in
[Docs](../../specs/pages/docs-pages.md#reusable-page-lifecycle),
[warm runtime](../../specs/preview-pipeline/warm-runtime.md#warm-docs-pool) and
[serving](../../specs/preview-pipeline/serving.md#preload-selection).
[Performance](../../specs/research/docs-navigation.md#warm-docs-pool-decision-2026-10-23)
records measured conditions and remaining timing/memory qualifications.

The shipping and installed confirmation below complete this lab. This archive
keeps the plan and reviewed evidence; product tests own regression coverage.
Reports contain reduced timing distributions and check outcomes, source hashes,
fixture shapes and measured host conditions. Console payloads are represented by
counts; raw logs, timeout snapshots, debug/smoke artifacts and executable runners
and variants are not retained. Reviewed Markdown reports preserve the console
comparisons and interpretation. Reduced JSON is evidence, not raw runner output.

## Shipping confirmation, 2026-10-23

The shipping implementation is measured as `control`, with no UI overlays;
the original variant definitions and earlier reports above describe their
measured sources. Use the same fixtures, independent readiness/correctness
probe, warm-up and navigation/edit sequences. Run a shipping smoke on Acme
and heavy, then three measured sessions per fixture after one discarded
session, with no additional cold cells (stage 2 found no cold-start gain).
The full run includes the 50-selection heavy walk and 10-second idle. Compare
the integrated values to stage 2 `pool-rp` as confirmation of the design;
without the old implementation alongside it this is not a new paired estimate
of the percentage gain. Source hashes include the new shipping host files.

The built host VSIX is also installed into an isolated VS Code profile and
opened on a temporary Acme copy: walk beyond pool capacity, return to a
retained page, check scroll and Show code, and count live Docs documents.
This is compatibility evidence, not an installed-host latency benchmark.
The installed runner scopes the standalone probe to the selected canvas
iframe; its initial attempt retained the probe's top-window guard and failed
before any page assertion. That failure was a runner defect.
Further installed attempts established that the window must be foreground for
animation-frame observations, and that Acme's smooth scrolling must settle
before the runner saves its scroll position. The final VSIX also restores page
positions immediately regardless of smooth-scroll CSS. The
[final installed report](results/2026-10-23-installed-final/report.json) passes;
the earlier runner failures are summarized here.

The three-session shipping run completed with every check passing. Final
compatibility review then added explicit application of the existing page-state
bridge after a new start and a Markdown swap; browser tests reproduce and cover
root attributes and conditional Markdown, including a return to the default
state. A final source confirmation repeats the same full sequences with one
discarded and one measured session per fixture (`--sessions 1 --cold 0`),
including the 50-selection walk. Its smaller timing sample supplements the
three-session report; it does not replace it or establish a new percentage gain.

## Shipping reports, 2026-10-23

- Smoke: all checked behavior,
  parity and cleanup passed on Acme and heavy; not a timing decision sample.
- [Three measured sessions](results/2026-10-23-shipping-confirmation/README.md):
  first-visit p50 63/79 ms on Acme/heavy, retained return 28/30 ms; 21.8 MB
  settled heavy-fixture JS heap with four documents. Every check passed.
- [Final source pass](results/2026-10-23-shipping-final/README.md), after applying
  the page-state bridge: first-visit p50 63/79 ms, retained return 28/30 ms;
  21.9 MB settled heap and four documents. Every check passed. One measured
  session per fixture limits tail interpretation.
- [Reviewed installed check](results/2026-10-23-installed-reviewed/report.json):
  actual packaged VSIX, isolated VS Code profile and Acme copy; walks beyond
  pool capacity, retains scroll, opens/colours source, stays at four documents,
  and applies Button's disabled state to both page options and HTML. Earlier
  installed failures are runner diagnostics (canvas scope, focus, smooth scroll),
  not passing product assertions.

Results match the adopted warm-pool design's scale. They do not establish a
new paired percentage gain: only shipping code runs in these sets, and host
load was lower than stage 2. The six-sample heavy edit tails in the larger
run remain exploratory. [Performance](../../specs/research/docs-navigation.md#warm-docs-integration-2026-10-23)
owns the product interpretation. The final scoped frame test also covers
clearing frozen idle dimensions before mounting a new destination; timing uses
the fixed lab viewport.
