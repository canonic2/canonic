# Docs navigation and warm pool evidence

Dated observations and comparison evidence. [Performance](../system/performance.md)
owns requirements and measurement semantics; [research ownership](README.md)
defines this record's evidence limits. Measurements below retain their original
conditions and do not establish acceptance for an untested build or host.

## Acme docs navigation, 2026-10-19

On macOS x64, an isolated server selected `acme` from the root `workbench.yaml`.
The repository's headless Chrome harness used a 1900 × 1100 viewport and opened
Colors, Typography and Button through the `html` docs lens, then returned to each.
At public base `3e2a86e`, first visits to Typography/Button took 261/286 ms;
with entry-script preloads they took 215/170 ms. Resource traces confirmed
parallel chunk fetches during the stylesheet wait, with one fetch per chunk.
These single samples demonstrate the waterfall correction, not a stable speed
ratio. Fresh-server Colors startup varied from 1.4 to 2.9 seconds, including
compiler startup. Retained returns took 17–35 ms and reused their documents.
These samples predate the 100 ms loading-feedback grace period.
This browser probe does not establish VS Code webview latency or other hosts.

## Loading feedback, 2026-10-20

The
[retained-session browser smoke](../../scripts/smoke-preview-sessions.cjs)
returned to its existing docs document in 22–24 ms on macOS x86_64 and observed
no frame painting the loading message. Its delayed Storybook lookup/render,
failure, supersession and capture checks also passed. Controller tests use a
controlled clock to verify the 100 ms feedback deadline and cancellation on
completion, replacement, failure and disposal. These browser checks do not
establish installed VS Code latency.

## Docs readiness handoff, 2026-10-21

The user's installed 0.14.1 Acme trace (16:12 local time, macOS x86_64)
showed repeated selection-to-visible times around 97–104 ms for Button, Icons,
Spacing, Overview and subsequent Colors/Typography selections. Document requests
took about 4–5 ms. The content renderer checked the docs readiness flag every
50 ms after document load, even though docs already emitted `workbench:ready`.
This added a wait between actual completion and promotion. Examples, bundle
loading and fonts accounted for additional work; Overview's syntax highlighting
completed after visibility and was not its visibility blocker.

A bounded correction listens to the document's existing readiness event,
rechecks its flag, cancels the pending poll and schedules promotion. It retains
the fallback for non-signalling renderers and the existing resource/readiness
requirements. Focused lifecycle tests cover the signal, premature signals,
fallback, replacement and disposal.

An isolated Chrome probe against that running Acme server compared installed
source with the corrected content-frame script intercepted in the probe only.
Four fresh tabs ran in installed/corrected/corrected/installed order, each
selecting Overview, Spacing, Typography, Colors, Icons, then Overview again
(12 selections per variant). Tracing and interception were enabled equally.
Nearest-rank p50 from the docs ready event to frame promotion was 27.6 ms
installed versus 1.4 ms corrected; selection-to-visible p50 was 146.5 versus
127.1 ms. Every observed promotion had its readiness flag set and fonts loaded;
the final probe reported no runtime exceptions. These are small diagnostic
samples, with uncontrolled host load, not an installed-webview latency claim or
evidence for a 4 ms end-to-end target.

The readiness correction does not change warm-up, retention or
font/image/example readiness. The subsequent
[warm Docs decision](#warm-docs-pool-decision-2026-10-23) defines the dedicated
host and pool that replace separate-document navigation for eligible new pages.
The [shipping integration evidence](#warm-docs-integration-2026-10-23) records
the implemented path and its measured limits.
The comparison exercised Acme's HTML docs, not an Astro-versus-docs benchmark.

## Warm Docs pool decision, 2026-10-23

Adopt the warm Docs pool with immediate retained resume and parallel page/metadata
fetching (`pool-rp`), with a background revision check on return. The goal is
about 40% faster browser-warm selection to visible for new Docs destinations
when an initialized pool document can be reused, while retained returns stay
as fast as separate-document retention and every Docs behavior remains intact.
This is not a cold-start or universal latency claim. Pool ownership, cleanup,
freshness and bounds belong to [warm runtime](../preview-pipeline/warm-runtime.md#warm-docs-pool);
page behavior and compatibility belong to [Docs](../pages/docs-pages.md).

Evidence is the [stage 2 report](../../labs/warm-docs/results/2026-10-23-stage2/README.md)
and [lab plan and profile](../../labs/warm-docs/README.md#stage-2-profile-report-2026-10-23).
The comparison used a standalone server and headless Chrome 155 on macOS x64
(16 logical CPUs, 64 GB, AC power), Node 24.12.0, three measured sessions per
variant/fixture after one discarded session, browser-warm Acme HTML docs and
the generated heavy React/React Native Web fixture. Acme fonts were replayed
from one recording; local heavy-fixture fonts, inputs, readiness and retention
were fixed across variants. Selection to visible was observed by the runner's
own page/revision, examples, font and visible-image checks, not just the product
ready flag. Host load was uncontrolled (session-start 1-minute load p50 12.88,
maximum 58.83).

The report records first-visit p50 126 → 78 ms on Acme and 134 → 80 ms on heavy
(about 38–40%); evicted-return p50 110 → 62 and 156 → 80 ms; retained-return
p50 26 → 27 and 29 → 29 ms. Every prototype correctness check passed. The
owner accepts the measured extra JS heap: heavy-fixture 50-page walk plus
10 seconds idle, four documents, about 10.3 → 22.8 MB. This is a deliberate
exception to the lab's original heap-parity rule, not evidence of a universal
memory ceiling or permission for heap to grow without bound. The optional
registry cleanup did not reduce that cost; the earlier-promotion variant widened
tails and is not adopted.

The candidate did not mechanically pass every original timing threshold:
heavy retained-return p95 was 38 versus 30 ms and Acme example-edit p95 was
199 versus 170 ms, with only six edit samples per fixture/variant. Host load
and those small samples limit attribution; they do not prove regressions absent.
The background return revision check was not part of the measured immediate-resume
variant. Shipping-code confirmation must measure its cost as well as confirm
freshness after a missed update. Run the same navigation, lens/design round
trips, edit, interaction, capture/parity, isolation and long-walk sequences
against integrated code, then confirm Acme in installed VS Code. Prototype
numbers do not establish shipping performance or host compatibility.

The server's initialized-runtime preload fix ships first as an independent
change covering previews and Docs ([serving](../preview-pipeline/serving.md#preload-selection)).
The pool and background checks are implemented. Compiler pre-warming and
readiness boundaries are unchanged; integration evidence follows.

## Warm Docs integration, 2026-10-23

The shipping UI implements the dedicated Docs pool and reusable page lifecycle.
The server serves its authored HTML host unchanged, omits initialized runtime
preloads, and supplies separate Markdown/compiled-example revision identities.
The latter distinguishes prose-only refreshes from missed example builds and
detects a save between parallel HTML and metadata reads. The compiled revision
uses the entry's reachable module content hashes, errors and compiler generation;
shared chunk layout and unrelated entries do not change it. Successful hot updates
advance the live page's examples revision before subsequent Markdown checks.
A project stylesheet's smooth-scroll setting does
not animate programmatic page reset or restoration.

Focused Rust client/page/service/export and asset-delivery tests pass. Node
serving tests verify authored assets and portable bootstrap; a real compiler
test confirms an example edit changes the return revision without a Markdown
save. Real Chrome tests cover page start/stop, mount cancellation/failure,
isolation, revision races/failures, parallel-response consistency, smooth-scroll
restoration and disposal. Canvas/session tests cover reuse and shared retention
bounds. These are scoped checks, not the package's full engineering gates.

The [shipping smoke summary](../../labs/warm-docs/README.md#shipping-reports-2026-10-23)
passed every checked behavior on Acme and the generated heavy fixture, with
no session or cleanup failures. Standalone capture preparation reports 503
without a capture helper; its screenshot/readiness parity is not native-capture
acceptance. Expected fixture errors and missing optional local config/favicon
requests remain separate from runtime warnings. No unused-runtime-preload
warning was observed. The smoke predates the immediate scroll-restoration fix
and has too few samples to establish the timing gain.

The [reviewed installed VSIX report](../../labs/warm-docs/results/2026-10-23-installed-reviewed/report.json)
passes on VS Code 1.142.0 / Electron 43.7.7 / Chromium 150, macOS x64, using
an isolated profile and temporary Acme copy. It walks beyond pool capacity,
returns with scroll preserved, opens and colours Show code, and observes four
Docs documents, and verifies Button's disabled state in page options and the
HTML state attribute. Installed file/binary hashes are recorded. This is compatibility
evidence, not an installed-host latency benchmark or cross-platform acceptance.
Earlier installed attempts exposed runner assumptions about nested canvases,
foreground animation frames and Acme's smooth scrolling; their failed reports
remain in the lab record.

The [three-session confirmation](../../labs/warm-docs/results/2026-10-23-shipping-confirmation/README.md)
passes every checked behavior and cleanup assertion. First-visit p50 is 63 ms
on Acme and 79 ms on heavy, retained returns 28/30 ms, and evicted returns
62/65 ms. Heavy's 50-selection walk plus 10-second idle settles at 21.8 MB JS
heap with four documents. This matches the selected warm-pool design's scale;
host load is lower (session-start p50 6.53 versus stage 2's 12.88), so it is
not a new paired estimate of the percentage gain. Heavy edit p95 values of
396 ms for Markdown and 262 ms for examples have only six samples each;
tail parity is not established by that sample.

The [final source confirmation](../../labs/warm-docs/results/2026-10-23-shipping-final/README.md)
repeats the full sequences after the state-bridge correction, with one
discarded and one measured session per fixture. Every check and cleanup
assertion passes. First-visit p50 remains 63/79 ms and retained return p50
28/30 ms; settled heap is 21.9 MB with four documents. It supplements the
larger run, with the [dated method](../../labs/warm-docs/README.md#shipping-confirmation-2026-10-23)
and source hashes recorded separately. A final scoped frame test additionally
verifies that a reused idle document drops frozen dimensions before mounting
at the current viewport; the timing runs used fixed viewport dimensions.
