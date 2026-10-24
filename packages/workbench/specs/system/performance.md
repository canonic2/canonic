# Performance

## Purpose and scope

Pages in Workbench's native formats must render quickly and stay quick while
someone moves between them. Workbench discovers, compiles, serves and mounts
these pages itself, so it is responsible for the whole wait between a click and
a usable page, not only for its own interface.

**Native formats** are the page formats Workbench defines:

- **TypeScript previews:** `definePreview` definitions in `*.workbench.ts` and
  `*.workbench.tsx`, shown as a page's design lens or through a `workbench`
  lens; see [TypeScript previews](../pages/previews.md).
- **Docs:** Markdown pages, `defineDocs` definitions and docs lenses, with
  their examples rendered by an adapter; see [docs](../pages/docs-pages.md).

The word refers to Workbench's own formats. It is unrelated to React Native
previews, the native window stream, or native capture.

For other page sources Workbench does not control the render. Authored HTML
pages, URL and Storybook implementations, the iOS Simulator and window
streams are measured for the time Workbench adds, such as the proxy, frame
handling and readiness. Their own load and render time belongs to the
project or the external tool.

This spec owns the performance requirements and how they are measured. The
other specs in the [preview pipeline](../preview-pipeline/README.md) define how the pipeline meets
them. [Labs](labs.md) owns how candidate implementations are compared.
Feature specs own the behavior that must not change while making it faster.

## Requirements

### Work is shared per project

- A space root has one compiler per profile in the server, and at most
  one Node evaluator, shared by every native page in it. Opening another
  page, scenario, size or lens in the same space never starts another compiler.
- Code shared by a project's native pages is compiled once per project and
  adapter. Each compatible warm document retains its initialized shared runtime
  across pages; independently retained documents own separate mutable runtimes.
  Shared-code URLs stay stable while their content is unchanged, so the browser's
  HTTP and code caches keep them across documents, pages and reloads.
- The cost of opening a page grows with that page's own code, not with the
  size of code it shares with other pages or the number of pages in the
  project.
- Workbench may prepare work ahead of a click, such as compiling likely next
  pages or keeping a loaded spare runtime, when that work stays within the
  resource bounds below and never delays the page that was asked for.

### Navigation keeps the canvas usable

- Sidebar selection changes immediately when a page is picked. Canvas replacement
  and loading feedback follow the [200 ms navigation grace period](rendering-flow.md#navigation-feedback):
  keep the outgoing view under its actual labels and context until the destination
  is ready or the deadline expires. A ready destination is revealed immediately;
  there is no minimum display delay. If still pending at 200 ms, conceal the outgoing
  view and show loading feedback. Pending navigation blocks preview interaction,
  screenshots and handoffs. Completion, failure, supersession and disposal cancel
  the feedback timer, so a fast return cannot flash a late loader.
- Changing the scenario, size, lens or inputs of a native page does not reload
  its compiled code. A scenario or input change re-renders in place; a size
  change resizes the frame.
- Returning to a page the canvas still retains shows it without compiling,
  loading or mounting again. Docs resume immediately and check freshness in
  the background; refresh only when the revision changed, under the
  [warm Docs contract](../preview-pipeline/warm-runtime.md#resume-and-freshness).
- Retain the previous page's live state until its replacement is ready. During
  the grace period its displayed labels, geometry and reference still identify
  that outgoing page. Publish the new artboard identity with its ready content,
  as [rendering flow](rendering-flow.md) requires.
  A new selection cancels an older one that is still loading, and the
  older one never replaces the newer.
- Readiness waits only for what a capture needs: the render, fonts and
  visible images. It adds no fixed delays, and its time is measured
  separately so that a slow wait is visible.

### First open and edits

- Missing preview and docs entry chunks preload together while their stylesheets load.
  They execute in dependency order after styles are available, so examples that
  read computed styles during initialization retain that behavior. A runtime
  already initialized for the profile is excluded from preloads as well as
  execution ([serving](../preview-pipeline/serving.md#preload-selection)).
- The page list appears from discovery without waiting for any page to
  compile. Discovery reads only what it needs to list pages.
- The complete page list of Workbench definitions (every page's title,
  group, scenarios and icon) shows on the canvas's first paint, not filled in
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
- The selected scenario, size and input overrides survive a hot update and a
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

The [page-index contract](../preview-pipeline/page-index.md#process-and-resource-ownership) owns
saved-index byte/count bounds, eviction and process limits. Compiler and warm
runtime bounds belong to their respective topics; an index bound does not
establish a bound for the entire pipeline.

## Measured interactions

Each interaction is measured from the user's action to the painted result, on
the reference fixtures defined by the [shared lab fixtures](../../fixtures/labs/README.md)
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
| Switch scenario or inputs | Command to re-rendered | Workbench overhead ≤ 50 ms |
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

The adopted warm Docs pool has a more specific improvement goal: about 40%
faster browser-warm selection to visible for new destinations when an initialized
idle document can be reused, with retained returns as fast as separate-document
retention. It is not a cold-start or universal latency target. The
[comparison record](../../research/docs-navigation.md#warm-docs-pool-decision-2026-10-23)
defines the measured conditions and limitations; [warm runtime](../preview-pipeline/warm-runtime.md)
owns the pool bounds and accepted memory cost.

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
events, correlated by page and scenario, so that a slow selection in someone's
editor can be broken down from the Workbench log without a profiler.

Enable `workbench.trace` and reload VS Code to collect these records in
**Output → Workbench**. Standalone hosts accept `--trace`. Both default to off.
`preview.request.completed` and `preview.browser.phase` share `selection`
(a navigation ID), page, scenario and lens where available. Local document and
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

The native page index and Rspack compiler are implemented. The eager compiler
still waits for the full entry set on first open; complete-list and first-page
target acceptance remains open. Scoped shipping Docs navigation and installed
startup checks do not establish the full format or platform matrix.

The shared-evaluator requirement is also incomplete: definition indexing can
start a short-lived evaluator in addition to the compiler's persistent one
([process model](server.md#processes-and-connections)). Literal-only listing
avoids that work; a React/HTML adapter alone does not guarantee every definition
or project config can be listed without JavaScript.

Dated measurements, fixtures, revisions and limitations live in
[research](../../research/README.md). The [page-listing record](../../research/page-listing.md)
records the large-fixture target miss; [installed startup](../../research/startup.md)
separates runtime preparation from listing. [Docs navigation](../../research/docs-navigation.md)
and [highlighting](../../research/docs-highlighting.md) distinguish prototype
comparisons from integrated and installed compatibility evidence.

### Pipeline acceptance

The [pipeline overview](../preview-pipeline/README.md#status) maps implemented capabilities to their
contracts. The full format-by-format navigation and edit confirmation through
the shipping server and VS Code webview remains open. Lab comparisons establish
only their recorded builds and conditions.

### First page

The shipping eager compiler does not meet the [first-open requirement](#first-open-and-edits):
the first page waits for all entries to be prepared and built, even with a warm
build cache. Native listing and the adopted native compiler do not change that
startup boundary. A replacement must preserve navigation, edits, errors and
isolation while meeting the same end-to-end budget.

## Verification

- Compare candidate approaches under the [lab methodology](labs.md).
- Confirm selected changes on reference fixtures through the shipping server
  and VS Code webview. Record measured values, cache states, host, fixture and
  source revision in the owning research record; link its result from the
  capability's status.
- Keep test coverage, observed correctness, latency and platform acceptance
  distinct. Research records do not redefine budgets or correctness tolerances.

## Open questions

- Which reference host should the targets assume, beyond Apple silicon macOS?
  Other platforms have no measurements.
- Should a repeatable performance check run before releases, and on which
  fixture?
