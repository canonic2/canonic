# Performance

## Purpose and scope

Pages in Workbench's native formats must render quickly and stay quick while
someone moves between them. Workbench discovers, compiles, serves and mounts
these pages itself, so it is responsible for the whole wait between a click and
a usable page, not only for its own interface.

**Native formats** are the page formats Workbench defines:

- **TypeScript previews:** `definePreview` definitions in `*.workbench.ts` and
  `*.workbench.tsx`, shown as a page's design lens or through a `workbench`
  lens; see [TypeScript previews](../previews.md).
- **Docs:** Markdown pages, `defineDocs` definitions and docs lenses, with
  their examples rendered by an adapter; see [docs](../docs-pages.md).

The word refers to Workbench's own formats. It is unrelated to React Native
previews, the native window stream, or native capture.

For other page sources Workbench does not control the render. Authored HTML
pages, URL and Storybook implementations, the iOS Simulator and window
streams are measured for the time Workbench adds, such as the proxy, frame
handling and readiness. Their own load and render time belongs to the
project or the external tool.

This spec owns the performance requirements and how they are measured. The
other specs in the [preview pipeline](README.md) define how the pipeline meets
them. [Labs](../labs.md) owns how candidate implementations are compared.
Feature specs own the behavior that must not change while making it faster.

## Requirements

### Work is shared per project

- A space root has one compiler per profile in the server, and at most
  one Node evaluator, shared by every native page in it. Opening another
  page, state, size or lens in the same space never starts another compiler.
- Code shared by a project's native pages is compiled once per project and
  adapter, and loaded once per browser session. Its URLs stay stable while
  its content is unchanged, so the browser's HTTP and code caches keep it
  across pages and reloads.
- The cost of opening a page grows with that page's own code, not with the
  size of code it shares with other pages or the number of pages in the
  project.
- Workbench may prepare work ahead of a click, such as compiling likely next
  pages or keeping a loaded spare runtime, when that work stays within the
  resource bounds below and never delays the page that was asked for.

### Navigation keeps the canvas usable

- Loading feedback has a 100 ms grace period per selection. A ready destination
  is revealed immediately, including during that period; only the loading
  message is deferred. Pending destinations remain busy and outgoing pixels
  remain concealed. Completion, failure, supersession and disposal cancel the
  feedback timer, so a retained return cannot flash a late loader.
- Changing the state, size, lens or inputs of a native page does not reload
  its compiled code. A state or input change re-renders in place; a size
  change resizes the frame.
- Returning to a page the canvas still retains shows it without compiling,
  loading or mounting again.
- Retain the previous page's live state until its replacement is ready. It may
  stay visible only under its actual identity; conceal it before showing the next
  destination's labels, as [rendering flow](../rendering-flow.md) requires.
  A new selection cancels an older one that is still loading, and the
  older one never replaces the newer.
- Readiness waits only for what a capture needs: the render, fonts and
  visible images. It adds no fixed delays, and its time is measured
  separately so that a slow wait is visible.

### First open and edits

- Preview and docs entry chunks preload together while their stylesheets load.
  They execute in dependency order after styles are available, so examples that
  read computed styles during initialization retain that behavior.
- The page list appears from discovery without waiting for any page to
  compile. Discovery reads only what it needs to list pages.
- The complete page list of Workbench definitions (every page's title,
  group, states and icon) shows on the canvas's first paint, not filled in
  progressively, and its time does not grow with the number of pages.
  The story lists of pages mapped to Storybook are outside this requirement.
- The first page compiles on request. Compilation after a server restart
  uses the persisted build cache when its inputs are unchanged.
- The time to the first page after the server starts does not grow with the
  number of pages in the project. The first page waits for its own
  definition and build, not for the whole page list or other pages' builds.
- An edit to a page's source or to code it shares rebuilds only what the edit
  affects, and the canvas shows the change without a manual refresh. A page
  whose inputs did not change is not rebuilt.

### Edits apply in place

- Native pages support hot module replacement: an edit to a module in a
  page's graph replaces that module in the running page without reloading
  its document.
- React components keep their state across edits through React Fast Refresh:
  editing a component re-renders it with its hooks' state preserved, as long
  as the edit keeps the hooks' order. An edit that Fast Refresh cannot apply,
  such as one that changes a module exporting more than components, re-mounts
  the affected tree.
- An edit that cannot be applied in place reloads the page. This includes
  the preview definition, the project or preview environment,
  `workbench.config.ts`, and modules with side effects that do not accept hot
  updates.
- The selected state, size and input overrides survive a hot update and a
  fallback reload.
- A syntax or runtime error during an update shows in the frame, and the next
  successful save recovers without a manual reload.
- Edits reach the page by a push from the server, not by polling, so their
  latency does not depend on a polling interval.

### Speed never costs correctness

A faster path must keep every existing contract, in particular:

- one page's module state, listeners, styles, request mocks and environment
  never reach another page;
- edits always reach the canvas, and an unchanged page is never served stale
  output;
- a page that fails to compile or throws shows its own error without breaking
  other pages in the space;
- screenshots and exports render exactly what the canvas shows.

### Resource bounds

Retained frames, the compilers' and the Node evaluator's memory, the in-memory and on-disk build
caches, and any prepared spare runtimes each have a stated bound and an
eviction rule in the owning feature spec. Speed achieved by unbounded
retention does not meet these requirements.

The [page-index contract](page-index.md#process-and-resource-ownership) owns
saved-index byte/count bounds, eviction and process limits. Compiler and warm
runtime bounds belong to their respective topics; an index bound does not
establish a bound for the entire pipeline.

## Measured interactions

Each interaction is measured from the user's action to the painted result, on
the reference fixtures defined by the [preview loading lab](../../labs/preview-loading/README.md)
and on Apple silicon macOS in the VS Code webview. The **large** fixture has
40 page previews sharing about 5 MB of dependencies and application code; the
**small** fixture has 5 component previews.

The targets below are **proposed, not agreed**, except the ones marked
agreed. They become requirements when the owner accepts them, with the
measured conditions recorded here.

The owner agreed the warm first-page and page-list targets on 2026-10-15.
They hold at any page count, not only on the large fixture. With a cold
cache, the first page shows in about its own build time.
For listing, warm means a valid saved index and complete ordered answer.
For first-page rendering, report the saved-index, build-cache and runtime
preparation states separately; one warm cache does not imply the others.

| Interaction | Boundary | Proposed target (large fixture) |
| --- | --- | --- |
| Switch to another page in the same space | Selection to visible | p50 ≤ 300 ms, p95 ≤ 600 ms |
| Return to a retained page | Selection to visible | p50 ≤ 100 ms |
| Switch state or inputs | Command to re-rendered | Workbench overhead ≤ 50 ms |
| Change size | Command to resized frame | No reload; Workbench overhead ≤ 50 ms |
| Reload the current page | Command to visible | p50 ≤ 600 ms |
| Edit a React component in the current page (hot update, state kept) | File write to visible | p50 ≤ 300 ms |
| Edit a page's own source that needs a reload | File write to visible | p50 ≤ 1 s |
| Edit code shared by every page | File write to visible | p50 ≤ 2 s |
| Open the first page after the server starts, build cache warm | Canvas opening to visible | **Agreed:** p50 ≤ 500 ms |
| Open the first page after the server starts, build cache cold | Selection to visible | p50 ≤ 5 s |
| List pages, saved index warm | Canvas opening to the complete page list | **Agreed:** ≤ 200 ms |

On the small fixture, no interaction may be slower than the shipping behavior
it replaces by more than 10%.

### Workbench time and project time

A page's wait has two parts: what Workbench does (discovery, compilation,
serving, loading code, frames and readiness) and what the project's code does
once mounted (rendering, data effects, its own network and fonts). The targets
cover Workbench time. Project time is measured and reported beside it, so
that a slow page can be attributed to the right owner, but Workbench does not
claim it. Development builds of a framework run inside project time.

### Diagnostics

Every selection of a native page records, per phase: server work time,
module load, mount, each readiness wait and frame promotion. They are
logged through the existing `preview.request.*` and `preview.browser.phase`
events, correlated by page and state, so that a slow selection in someone's
editor can be broken down from the Workbench log without a profiler.

Enable `workbench.trace` and reload VS Code to collect these records in
**Output → Workbench**. Standalone hosts accept `--trace`. Both default to off.
`preview.request.completed` and `preview.browser.phase` share `selection`
(a navigation ID), page, state and lens where available. Local document and
descriptor/bundle requests carry the ID as `wbTrace`; asset URLs and retained
session identity remain stable. The server's `Server-Timing` header exposes
its request durations in webview developer tools.

Browser records identify selection, content renderer readiness, document load
(with retained/warm flags), descriptor/bundle fetch, example mount, readiness
waits and frame promotion. `durationMs` is the interval since the previous
phase in the named `owner` (preview runtime records call this `phaseMs`);
`elapsedMs` uses the canvas selection start in the
canvas/renderer, and the document's own start in document/runtime records. The
canvas's `visible` record has `totalMs` from selection initiation to a paint
opportunity after publishing the selected canvas. Renderer/document phases
can overlap and must not be summed across owners. Returned documents report
their reuse instead of inventing fetch or mount work. Preview runtime readiness
records additionally split hooks, fonts and images. Superseded selections do
not report a successful visible total. Timing logs stop at the diagnostic
session budget and do not contain page bodies, input values or example source.

Tracing itself adds diagnostic requests and output work. Use it to locate
waits; controlled lab comparisons must declare its state and independently
observe the requested content becoming visible.

## Status

The native page index is implemented, while the complete-list and first-page
targets remain unmet at the largest measured page counts. Listing measurements
are below; the compiler's first-page limitation is recorded separately. The
implemented Rspack pipeline has correctness evidence, but its complete sequence
through the shipping server and VS Code webview has not been measured.

### Pipeline acceptance

The Rspack pipeline is implemented with shared chunks, pushed hot updates and
warm documents. Its correctness and design evidence are linked from the
[pipeline overview](README.md#status). Full navigation and edit sequences
through the shipping server and VS Code webview need a confirmation run;
prototype timings do not establish those product targets.

The [pipeline lab](../../labs/rspack-pipeline/README.md#stage-2-report) holds
controlled prototype comparisons. The [first-page lab](../../labs/first-page/README.md#report)
and [compiler-host plan](../../labs/instant-first-page/README.md#stage-2-plan)
address startup scaling. Their alternative implementations are not the active
compiler contract.

### Page listing

The server's [native page index](page-index.md) lists pages independently of
the compilers and the Node evaluator. Its saved-state, evaluation and failure contracts are
owned there. The measurements below distinguish the lab prototype from the
shipping-server boundary.

The [instant first page lab](../../labs/instant-first-page/README.md#stage-1-report)
holds the index design comparison. Process startup, adapter ordering and
configuration assembly belong to the shipping-server boundary below.
A newly placed macOS executable may also incur a one-time system launch check;
report that state separately from subsequent launches.

Shipping-server probe, 2026-10-15 at about 20:54 EDT: Node 24.12.0, Intel
macOS, release native executable, Acme seed 1. Each fixture had one cold read
and three warm reads, with a fresh server instance per read and no eager
pre-warming. Framework dependencies were not installed: listing evaluates
definitions and config, not their preview sources. The bundled runtime was
already extracted. One-minute host load was 7.79–8.05; these exploratory
samples are not a lab run set or a canvas-paint measurement.

| Definitions | Cold HTTP config | Warm native work | Warm `previews.indexed` | Warm `config.resolved` | Warm HTTP config |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 5 | 349 ms | 2 ms | 9 ms | 9 ms | 12 ms |
| 42 | 251 ms | 3 ms | 10 ms | 10 ms | 13 ms |
| 602 | 329 ms | 15 ms | 27 ms | 29 ms | 40 ms |
| 10,002 | 2,435 ms | 162 ms | 243 ms | 295 ms | 467 ms |

Warm columns are medians of the three reads. The large fixtures include two
broken previews, whose definitions still list. The 10,002-definition warm
HTTP responses ranged from 393 to 532 ms. Every config listed all definitions
without starting a compile worker; unchanged reads parsed and evaluated none.
Cold reads also evaluated `workbench.config.ts`, and the 10,002-definition
fixture evaluated 500 computed definitions. Runtime preparation uses a second
native pass, reusing literal results saved by the first pass.

Measured source: public base `6d9d0b5`, private base `bd86d29b`, plus the
uncommitted page-index implementation. SHA-256 prefixes identify the measured
files: server `aac7f3b3951f`, compiler `4e9f7f3cadc7`, evaluator
`e3c7efaaacb7`, TypeScript adapter `7a464917b48e`, native index application
`0b467d3c834e`, release executable `b5459121478c`.

A loaded-host repeat at about 21:00 EDT used the installed release's executable. One-minute host load was
13.27–20.16. Warm HTTP medians were 35, 44, 48 and 1,198 ms for the same four
fixtures; the 10,002-definition native work had a median of 181 ms and its
HTTP responses ranged from 596 to 1,499 ms. No compile worker started.
Single cold HTTP reads were 4,062, 3,373, 391 and 9,074 ms. Scheduling and
process startup dominate some samples; this loaded-host repeat is correctness
and diagnostic evidence, not a controlled speed comparison. Its measured
SHA-256 prefixes are adapter `9c0efd4704a6`, native application `c0189a1d7c12`
and executable `4ea1d096abc4`; server, compiler and evaluator match the first
probe.

The 200 ms complete-list target is not met on the 10,002-definition fixture.
Process startup, saved-answer transport, collection assembly and the full
config response add to native work. Canvas first paint and the VS Code
webview boundary require a separate measurement; these server probes do not
establish that target. The server's resident index removes the process
per request and, while nothing changed, the disk check; it has not been
measured yet ([server](../server.md#the-resident-index-and-watcher)).

### First page

The shipping eager compiler does not meet the
[first-open requirement](#first-open-and-edits): its first page waits for all
entries to be prepared and built even with the build cache warm. Native
listing does not change that compiler start or pre-warming. The Rust compiler
host remains lab work. A replacement must meet the first-page budget through
the shipping canvas and preserve page switches, edits, errors and isolation;
a faster list alone does not establish success.

## Verification

- Candidate approaches are compared in [labs](../labs.md): the
  [preview loading lab](../../labs/preview-loading/README.md), the
  [compiler speed lab](../../labs/compiler-speed/README.md) and the
  [Rspack pipeline lab](../../labs/rspack-pipeline/README.md).
- A change made for performance is confirmed on the reference fixtures
  through the shipping server and the VS Code webview, and the measured
  values, conditions and Workbench revision are recorded in this spec's
  status.

### Acme docs navigation, 2026-10-19

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

### Highlighting, 2026-10-20

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

### Loading feedback, 2026-10-20

The
[retained-session browser smoke](../../scripts/smoke-preview-sessions.cjs)
returned to its existing docs document in 22–24 ms on macOS x86_64 and observed
no frame painting the loading message. Its delayed Storybook lookup/render,
failure, supersession and capture checks also passed. Controller tests use a
controlled clock to verify the 100 ms feedback deadline and cancellation on
completion, replacement, failure and disposal. These browser checks do not
establish installed VS Code latency.

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

### Installed asynchronous highlighting, 2026-10-20

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

### Highlighting comparison, 2026-10-20

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

### Highlighting performance campaign, 2026-10-21

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

### Integrated asynchronous highlight.js, 2026-10-21

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

### Whole-project docs warm-up follow-up

Code-path inspection for tracing (2026-10-20) found that `docs_bundle()` waits
for the compiler's `warmed()` barrier before ensuring the requested lens.
`Space::warm_docs` also lists and requests every docs lens sequentially. These
are separate startup paths, so an on-demand comparison must account for both.
The content frame readiness observer retries at 50 ms intervals; the canvas
defers loading feedback for 100 ms, independently of readiness. These
are inspection findings, not VS Code latency measurements.

After collecting installed-extension traces, plan a lab under `labs/` following
[the lab contract](../labs.md). Compare shipping warm-up with on-demand Markdown
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

## Open questions

- Which reference host should the targets assume, beyond Apple silicon macOS?
  Other platforms have no measurements.
- Should a repeatable performance check run before releases, and on which
  fixture?
