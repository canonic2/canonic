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
The shell still shows its loading status immediately, including on fast returns.
This browser probe does not establish VS Code webview latency or other hosts.

## Open questions

- Should a page become visible when it mounts, with full readiness kept for
  captures? That would trade a brief flash of unloaded fonts and images for a
  shorter wait ([readiness](readiness.md#open-questions)).
- Which reference host should the targets assume, beyond Apple silicon macOS?
  Other platforms have no measurements.
- Should a repeatable performance check run before releases, and on which
  fixture?
