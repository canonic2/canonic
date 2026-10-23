# Docs highlighting evidence

Dated observations and comparison evidence. [Performance](../system/performance.md)
owns requirements and measurement semantics; [research ownership](README.md)
defines this record's evidence limits. Measurements below retain their original
conditions and do not establish acceptance for an untested build or host.

## Highlighting, 2026-10-20

The HTML and TypeScript waits come mainly from first-use grammar regex
compilation. The syntax set is shared, but a first HTML fence does not compile
the TypeScript grammar needed by Show code. The installed Acme trace recorded
43 ms highlighting its HTML fence, then 448 ms highlighting the example's
TypeScript. This does not imply that every request costs that much.

Temporary optimized probes compared Syntect 5.3.0 with two-face 0.4.5 using
`regex-fancy`/`syntect-fancy` and `regex-onig`/`syntect-onig`. They used the
shipping highlighter (source SHA-256 prefix `ebfb3078d18c`) with a no-op trace
guard and clocks around `parse_line` and scope classification. Inputs were the
HTML fence from `demo/system/overview.md`, then the `tokensInUse` export from
`demo/system/overview.examples.ts`. Each backend ran in ten fresh processes,
alternating order, with one first use and eleven repeated calls per input.

| Input | fancy first-use p50 | Oniguruma first-use p50 | fancy warm p50 | Oniguruma warm p50 |
| --- | ---: | ---: | ---: | ---: |
| HTML fence | 28.30 ms | 4.39 ms | 0.308 ms | 0.209 ms |
| TypeScript example, after HTML | 368.68 ms | 20.65 ms | 0.837 ms | 0.293 ms |

Warm cells have 110 samples each. First-use maxima were 30.38/4.63 ms for
HTML and 412.99/21.01 ms for TypeScript (fancy/Oniguruma). Parsing accounted for
almost all the difference; scope classification took 0.03–0.08 ms. Conditions:
macOS 15.8.1 x86_64, iMacPro1,1, 16 logical CPUs, 64 GiB RAM, Cargo/Rust 1.99.1,
release optimization, host load about 6 with other engineering work active.
These are focused probes, not controlled browser or installed-webview targets.

The backend is Oniguruma, compiled into the server with shipped bindings.
It reduces on-demand work without pre-highlighting the page set. A 31-input
comparison covered the Acme docs sources and supported-language snippets:
all 18 Acme inputs produced byte-identical markup. Two synthetic JavaScript
and JSX snippets had token-class differences; the JSX closing tag is treated
as a tag rather than a regexp. Source text and escaping are preserved.

A second comparison used actual release servers built with the package's pinned
`nightly-2026-04-16` toolchain (binary SHA-256 prefixes: fancy `40ff75cbb697`,
Oniguruma `e1bce37d557`). Three fresh processes per backend alternated order on
the same host. Each used a temporary copy of Acme's system docs and assets with
only Overview configured, its HTML examples lens, and no project stylesheet.
The examples bundle was fetched first to settle startup/catalog/compiler work
without initializing the highlighter. The probe then fetched the Markdown
document and `tokens-in-use` Show code endpoint, each once cold and three times
warm. The first document and source responses were byte-identical between
backends. No Workbench build or test suite ran during these measurements;
other host work was not controlled.

| Request | fancy cold server p50 | Oniguruma cold server p50 | fancy warm server p50 | Oniguruma warm server p50 |
| --- | ---: | ---: | ---: | ---: |
| Overview document | 57.17 ms | 9.18 ms | 2.49 ms | 1.23 ms |
| TypeScript Show code, after document | 1174.28 ms | 33.62 ms | 4.17 ms | 1.94 ms |

Cold cells have three samples each; warm cells have nine. The corresponding
Oniguruma highlighting medians were 7.70/0.43 ms for the document and
32.40/0.59 ms for Show code (cold/warm). Loopback HTTP wall-time medians were
10.70/2.14 ms and 34.65/2.89 ms. These release request measurements include
the real docs service and tracing; they are distinct from the isolated probe
and the earlier installed trace. They do not measure browser visibility or
establish the performance targets. Docs still wait for examples and fonts
before appearing.

## Follow-up docs lab inputs

Live docs highlighting is deferred: escaped code is readable in the initial
document, and a document-owned local highlight.js worker applies colours without gating page
visibility. CSS, fonts, images and examples still gate readiness. Capture waits
for colouring to finish or fall back to plain text. The
[highlighting lab](../../labs/docs-highlighting/README.md) compares engine
choices against the deferred Syntect baseline. The owner selected asynchronous
highlight.js on 2026-10-21 after reviewing the results. The integrated path starts
its worker after document load and two animation frames and paints CSS Highlight
ranges over unchanged text nodes. The earlier synchronous measurements and the
prototype performance guardrail results below remain investigation history.

Verification on 2026-10-20: the
[browser check](../../ui/src/modules/docs/page/highlighting.test.js) held local
fence and Show code colouring responses open while the shipping docs script
reported ready and displayed exact plain source. On completion, code block bounds
stayed identical. It also checked edited/disposed content and failure fallback.
[Capture checks](../../ui/legacy/annotations-capture.test.js) verify that pixel
reads wait for colouring. These are behavioral checks in headless Chrome on
macOS x86_64, not installed-webview latency measurements or performance targets.

## Installed asynchronous highlighting, 2026-10-20

A focused behavioral check used the installed Workbench 0.14.0 release in
VS Code 1.142.0 (Electron 43.7.7, Chromium 150.0.7871.250) on macOS x86_64.
A separate VS Code process used a temporary profile and a copy of Acme's
system docs/assets, configured with Overview and Colors and their HTML examples
lens. It loaded the installed extension without changing its files or the user's
active window. Tracing was enabled; no lab variant or latency samples were run.

DevTools held the actual local fence and Show code colouring responses. Overview
became ready and visible through the full shell/content/docs frame chain with
plain fence text, loaded fonts and its mounted example. Show code displayed
plain TypeScript while both colouring responses remained held. Highlighting's
completion promise remained pending. Releasing both responses applied colours;
the fence and source text matched the fixture files exactly, and both code
bounding rectangles were unchanged. This establishes the installed asynchronous
visibility path and completion signal for this fixture. Pixel capture, export,
failure/timeout injection and the broader language matrix were not checked in
this installed smoke; their existing behavioral checks and lab confirmation
remain separate evidence.

The installed docs JS, highlighting JS, CSS and release binary matched the
workspace's packaged files byte for byte. SHA-256 prefixes: highlighting
`3d61624d7a83`, docs script `dd04ae30e2b3`, CSS `abcca825afdd`, release binary
`5663597d8c69`; public base `6494125a13bb`, private base `16e143e52c77`, with
uncommitted changes. The temporary structured verification report contained
separate visibility, plain-source, pending-completion, text and geometry checks.

Preflight discovery: the temporary VS Code process initially remained in the
background. Its canvas and descendants reported `visibilityState: hidden`, and
animation-frame probes did not fire within one second. DevTools
`Page.bringToFront` did not make them visible. Focusing that specific native
process changed all documents to `visible` and animation frames progressed;
the foreground verification then passed. A lab must independently establish
foreground visibility before measuring. The earlier temporary fixture also
needed a block YAML list for styles, rather than an inline list; neither setup
failure is a product latency sample.

## Highlighting comparison, 2026-10-20

The [lab report](../../labs/docs-highlighting/results/2026-10-20-preflight/README.md)
records asynchronous Syntect, document-owned highlight.js and TextMate/Oniguruma
workers, and a plain diagnostic through the shipping canvas. All four passed
isolated installed-VS-Code and portable compatibility smokes. This does not
establish installed-webview latency or integrated worker implementations.

The defined language-role corpus found an uncoloured SCSS variable in Syntect;
the workers passed. Source text and outer code bounds matched, but raster
differences remained, and a later character-range diagnostic measured subpixel
bound differences when spans were applied, including in the control. Unchanged
block bounds alone do not establish unchanged character geometry. One Markdown-edit
timeout in the highlight.js variant remains unresolved despite a passing diagnostic repeat.

Extreme host load interrupted the preflight timing attempt before the two full
sets and confirmation. Those diagnostic timings alone established no winner or
accepted target. The correctness findings remain distinct from the subsequently
completed performance comparison below.

## Highlighting performance campaign, 2026-10-21

The [complete report](../../labs/docs-highlighting/results/2026-10-21-performance/README.md)
records 1,200 attempted trials across two full sets and a control/highlight.js
confirmation, using the release server and shipping canvas in standalone Chrome.
Inputs, warm-up and retention stayed fixed. Aggregates were independently
recomputed, including actual counts, failures, phase observations and memory.

highlight.js repeatedly improved dense first-page colouring beyond the proposed
gain threshold and control variation. It also slowed dense second-page
visibility and smaller-page colouring beyond the latency allowance. TextMate
was slower on motivating/dense first-page colouring and missed the settled RSS
allowance in one cell. Neither prototype meets the overall performance rule;
the lab recommended retaining asynchronous Syntect. The owner subsequently
selected asynchronous highlight.js, accepting that engine choice despite the
prototype's overall guardrail result. This decision does not change the recorded
measurements, performance targets or correctness tolerances. Production uses
deferred worker creation and CSS Highlight ranges to address load-event blocking
and span-induced text shaping; prototype timings do not establish their latency.

All samples remain, including six execution failures and substantial host-load
variation. A quieter third set repeated the colouring gain and visibility
regression. Exact figures, nearest-rank tails, qualifications and unresolved
geometry/raster, role and freshness findings belong to the linked evidence.
Installed compatibility and the asynchronous visibility path were checked
separately; these timing results do not establish installed-webview latency.
The whole-project docs warm-up comparison remains a separate, unrun lab.

## Integrated asynchronous highlight.js, 2026-10-21

The [integration checks](../../labs/docs-highlighting/results/2026-10-21-integration/README.md)
cover the selected local worker and CSS Highlight range painter. Focused browser
checks preserve exact text/copy bytes and per-character geometry while testing
held work, fallback, cancellation and capture-mirror range transfer. Installed
VS Code checks with the release server pass for Overview and Code-heavy: the
latter checks 137,983 code characters and 12,011 ranges, with identical geometry
and a successful native capture. A nine-page portable export check also passes.
The standalone shipping canvas passes the navigation, retained/evicted return,
source/copy, edit, supersession, failure/recovery and capture sequences for all
three fixtures, retaining the lab's warm-up cycles.
Three hidden-canvas setup timeouts preceded the final installed checks; explicit
editor-tab activation resolved them. These are compatibility checks, not a new
performance matrix or captured-pixel parity claim. Prototype timings above do
not measure the integrated painter.

## Whole-project docs warm-up follow-up

Code-path inspection for tracing (2026-10-20) found that `docs_bundle()` waits
for the compiler's `warmed()` barrier before ensuring the requested lens.
`Space::warm_docs` also lists and requests every docs lens sequentially. These
are separate startup paths, so an on-demand comparison must account for both.
The content frame readiness observer retries at 50 ms intervals; the canvas
defers loading feedback for 100 ms, independently of readiness. These
are inspection findings, not VS Code latency measurements.

After collecting installed-extension traces, plan a lab under `labs/` following
[the lab contract](../system/labs.md). Compare shipping warm-up with on-demand Markdown
HTML plus only the selected page's example bundle on a large docs fixture.
Keep cold/warm build caches, syntax-set first use, first visit during startup,
first visit after warm-up, retained return and evicted return separate. Measure
click-to-visible, each server/browser phase, loader exposure, compile work,
peak/settled server and evaluator RSS, and retained browser memory. Check docs,
highlighting, examples, edits and supersession for equivalent behavior. No lab
runner or variant is part of the trace implementation, and no performance
target is established by this instrumentation alone.

A trace smoke on 2026-10-20 used the debug server, headless Chrome and macOS
15.8.1 x86_64, with tracing enabled. A temporary Acme fixture had two Markdown
pages sharing one HTML lens (`basic` writes text), and one TypeScript code
fence on the first page. Single selection-to-visible samples were 556 ms,
182 ms and 28 ms for first page, second page and retained return. The first
document spent 333 of its 335 server milliseconds highlighting; the deferred
bundle's warm barrier was below 0.01 ms. The return kept the same document.
This isolates first-use highlighting as a lab cell; it does not establish
typical navigation latency, memory, VS Code behavior or performance targets.
Measured uncommitted source SHA-256 prefixes: highlighting `ebfb3078d18c`,
request timers `cab09b9b7bae`, browser trace helper `0a93d59ca5d9` (before owner
labels were added). The fixture and trace are a smoke check, not a lab run set.
