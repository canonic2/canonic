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

- A space root has one compile worker, shared by every native page in it.
  Opening another page, state, size or lens in the same space never starts
  another compiler.
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
- The previous page stays visible and responsive until its replacement is
  ready. A new selection cancels an older one that is still loading, and the
  older one never replaces the newer.
- Readiness waits only for what a capture needs: the render, fonts and
  visible images. It adds no fixed delays, and its time is measured
  separately so that a slow wait is visible.

### First open and edits

- The page list appears from discovery without waiting for any page to
  compile. Discovery reads only what it needs to list pages.
- The first page compiles on request. Compilation after a worker restart
  uses the persisted build cache when its inputs are unchanged.
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

Retained frames, the compile worker's memory, the in-memory and on-disk build
caches, and any prepared spare runtimes each have a stated bound and an
eviction rule in the owning feature spec. Speed achieved by unbounded
retention does not meet these requirements.

## Measured interactions

Each interaction is measured from the user's action to the painted result, on
the reference fixtures defined by the [preview loading lab](../../labs/preview-loading/README.md)
and on Apple silicon macOS in the VS Code webview. The **large** fixture has
40 page previews sharing about 5 MB of dependencies and application code; the
**small** fixture has 5 component previews.

The targets below are **proposed, not agreed**. They become requirements when
the owner accepts them, with the measured conditions recorded here.

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
| Open the first page after the server starts, build cache warm | Selection to visible | p50 ≤ 1.5 s |
| Open the first page after the server starts, build cache cold | Selection to visible | p50 ≤ 5 s |
| List pages | Discovery request to page list | ≤ 500 ms |

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

Every selection of a native page records, per phase: worker queue and work
time, module load, mount, each readiness wait and frame promotion. They are
logged through the existing `preview.request.*` and `preview.browser.phase`
events, correlated by page and state, so that a slow selection in someone's
editor can be broken down from the Workbench log without a profiler.

## Status

These requirements are not yet met for TypeScript previews on large projects.
An exploratory investigation on 2026-10-11 of one selected local React project
with Workbench 0.13.1 found the following. Details are in the
[preview loading lab](../../labs/preview-loading/README.md#hypothesis).

- Switching to a page not yet opened took about 0.85–2.9 s, most of it in the
  browser. Cached server-side work was 12–26 ms per request.
- Each preview compiles to its own complete bundle (about 5.1 MB), and a newly
  selected page usually gets a new frame document, so shared code is loaded
  and evaluated again for every page. About 95% of each bundle was code
  shared with the other page.
- After mount, the events logged as the font and image readiness phases
  took about 0.5–0.8 s and 0.65–1 s. The log puts each phase's duration on
  the next event, so these may not be the font and image waits themselves
  ([gap](../previews.md#implementation-gaps)). A probe on 2026-10-12 in headless
  Chrome measured about 130 ms for each wait on a page with no web fonts
  ([readiness](readiness.md#discoveries)).
- Cold compiles took 2.7–11 s per page. Discovery requests took 0.5–1.6 s.
- Returning to a retained page took 82–133 ms, within the proposed target.
- Edits are not applied in place. Each open preview polls a revision URL once
  a second, and each poll asks the worker to check the preview's build; a
  changed revision reloads the whole preview. There is no hot module
  replacement or React Fast Refresh, and state other than the selected state
  and inputs is lost on every edit.

The [preview loading lab](../../labs/preview-loading/README.md)'s first run set
(2026-10-11, synthetic 40-page fixture, one Intel host, headless Chrome)
measured the control at 1.4 s to show a new preview, against 0.13–0.2 s with
a project-wide shared build and 0.34–0.48 s with the two HMR variants, whose
edits appeared in 0.05–0.16 s with React state kept. It also found that a
file edited during a compile can leave stale output cached
([TypeScript previews](../previews.md#implementation-gaps)). A second run set is
needed before a decision. The [compiler speed lab](../../labs/compiler-speed/README.md)
found Rspack 21–27% faster than esbuild on cold builds of the large fixture
and 26–71% faster on rebuilds, with built-in HMR and React Fast Refresh. It
also measured about 1.1 s of Workbench's own work around esbuild for each
cold preview.

Stage 1 of the [Rspack pipeline lab](../../labs/rspack-pipeline/README.md#stage-1-report)
(2026-10-12, formats fixture, one sample per edit, one Intel host, headless
Chrome) ran a prototype of the whole [preview pipeline](README.md). Every
edit requirement above was met on every native format: stylesheet, React,
Vue, HTML, Astro and docs edits showed in 51–189 ms with component state kept
where the framework keeps it, reloads took about 0.1 s, and errors recovered
on the next save, with no polling. These are feasibility samples, not
measurements against the targets. Stage 2 measures the targets on the large,
web-font and huge fixtures.

Stage 2 of the same lab (2026-10-13, two run sets, the same host) measured
the prototype against the targets above on the large, web-font and huge
fixtures ([report](../../labs/rspack-pipeline/README.md#stage-2-report)):

- New pages, back-to-back selections, reloads and every edit target were
  met by the warm document variant in both sets. New pages took 0.14–0.18 s
  at the median, against 0.4–2.6 s for the shipping pipeline.
- The warm document missed the retained-page target on the web-font fixture
  (109–114 ms) and the state-change target (143–168 ms), because it
  re-evaluates modules on those changes. The warm spare met both, but missed
  the new-page p95 once (992 ms).
- The first page after a restart with a warm cache took 3.3–3.4 s, against
  the 1.5 s target, because every entry rebuilds first. With pre-warming
  finished it took 0.81–0.83 s. Cold first opens took 6.0 s for the pipeline
  and 5.3–5.8 s for the shipping pipeline, both above the 5 s target.

Docs pages have not been measured for navigation, and page listing was not
measured.

The pipeline was implemented on 2026-10-13 ([preview pipeline](README.md#status)).
Its timings in the product are not measured yet: the lab's sequences through the
shipping server and the VS Code webview are the next step.

## Verification

- Candidate approaches are compared in [labs](../labs.md): the
  [preview loading lab](../../labs/preview-loading/README.md), the
  [compiler speed lab](../../labs/compiler-speed/README.md) and the
  [Rspack pipeline lab](../../labs/rspack-pipeline/README.md).
- A change made for performance is confirmed on the reference fixtures
  through the shipping server and the VS Code webview, and the measured
  values, conditions and Workbench revision are recorded in this spec's
  status.

## Open questions

- Should a page become visible when it mounts, with full readiness kept for
  captures? That would trade a brief flash of unloaded fonts and images for a
  shorter wait ([readiness](readiness.md#open-questions)).
- Which reference host should the targets assume, beyond Apple silicon macOS?
  Other platforms have no measurements.
- Should a repeatable performance check run before releases, and on which
  fixture?
