# Design-system export contract

The source-package export packages the selected pages as one download: the
source closure of each selected page, reference screenshots, a guide per page,
and, when the project has [TypeScript previews](../pages/previews.md), a portable
`browser/` viewer. It is a server-backed operation available from the editor
or a hosted browser; a `file://` workbench cannot export. See
the [export route](../../server/src/serve/routes/export.rs), the [export module](../../server/src/export.rs), and the
[capture contract](capture.md) for the renderer it uses. The user-facing
description is [Design-system export](../../docs/design-system-export.md).

**Status:** export jobs, the space-scoped source closure, the reference
capture plan (design scenarios, iterations, declared artboards, docs pages and
Storybook stories), ZIP, PDF, image and browser outputs, split archives and the
portable viewer are implemented. This topic records no unmet requirements;
cancellation and an overall job deadline are unspecified
([connection loss and cancellation](#connection-loss-and-cancellation)), and
coverage gaps are listed under [verification points](#verification-points).

## Module ownership

The export is the server's `export` module (`server/src/export/`), following
the [module layout contract](../system/modules.md): typed inputs and reports in
`types.rs`, filesystem-backed source closure and page guides in `source.rs`,
reference parsing in `references.rs`, and ZIP assembly and partitioning in
`archive.rs`. `build.rs` coordinates the optional portable build and source
archive, preserving a portable failure as a warning. `request.rs` validates
options and filters page scope; `selection.rs` filters reference variants;
`sizes.rs` decides the sizes a page is captured at; `paths.rs` holds the pure
path, URL, name and clock helpers;
`plan.rs` plans the references, `capture.rs` captures them through up to
four renderers, `output.rs` assembles PDFs and image/browser archives,
`jobs.rs` owns job lifecycle and retention, and `pipeline.rs` runs one
export. Tests live in each file.

The space supplies the capture helper (the host's, over its messages), the
docs pages, a Storybook's stories and the
[portable build](../../server/src/portable.rs)
([space/export.rs](../../server/src/space/export.rs)). The capture helper and the
compiler keep their own owners.

## Space scope

- One export targets a page, explicit pages, a named collection, or the whole
  current space. It does not aggregate other spaces listed in the switcher.
  The selection filters the resolved pages and collection hierarchy before
  source traversal, capture planning, and portable compilation.
- The export endpoint uses its server's selected configuration directory,
  space key, and root. It resolves that space's configuration and catalogs,
  then passes that resolved view to the source exporter, reference planner,
  and portable preview builder. It does not traverse the space registry.
- On a mixed-space canvas, the export action targets the selected artboard's
  owning space, as described under [capture scheduling](#capture-scheduling-versus-the-camera).
  The dialog chooses a scope within that space.
- The archive's page records, references, and portable catalog cover the
  target space. Shared imports and assets required by that space may also
  be used by other spaces; their inclusion follows the source closure.
- When the original `workbench.yaml` is within the exportable source roots,
  it is copied as written. It may declare several spaces; those declarations
  do not enroll the other spaces' pages in the export. A manifest outside
  those roots is omitted rather than synthesized as a single-space file.
- Exporting another space requires a separate request to that space's server.
  Export jobs, progress, retained archives, and the active-job limit belong
  to the target server; another space's job is not joined.

### Acceptance criteria

- With spaces A and B in one configuration, exporting A includes all A's
  pages and reference variants and excludes B-only page records and references.
- A page or collection export excludes unrelated page records, references,
  and portable previews; shared source dependencies remain included.
- Exporting B separately produces B's page records and reference plan.
- An included multi-space manifest retains its declarations without causing
  B's pages to enter A's export. Shared dependencies needed by A remain included.

## Source closure

Portable browser viewers include `browser/CANONIC-LICENSE.txt` for Canonic-owned
viewer code and generated authoring types. Project sources and third-party
components retain their own licensing.

- Export starts from every page's entry points in the target space: the design page, its code
  pointers, and each TypeScript preview's definition, source, and the local files its compiler
  resolves (including adapter and environment dependencies). It follows
  reachable local imports and assets and keeps project-relative paths.
  Sources outside the workspace go under `implementations/`.
- It includes the Storybook and package configuration the included sources
  need. Installed dependencies, build output, secrets, tests, and unrelated
  files are excluded. When the project has no root `package.json`, the export
  adds one listing the external packages the sources import. With previews
  and no `workbench-env.d.ts`, it adds one with the `@canonic2/workbench`
  types.

## Job lifecycle

- `POST /_workbench/export` starts a background job and answers `202` with
  its status. `GET /_workbench/export?job=<id>` reports progress, warnings,
  and, once complete, the download and its parts.
  `&download=1` downloads the completed bundle and `&part=<n>` one numbered
  part; an incomplete job answers `409`, and an expired one `404`. A `GET`
  without a job, for scripted callers, starts or joins a job, waits, and
  downloads.
- Only one export runs at a time per space server. Identical normalized requests
  join it; a different request answers `409`. A settled job is kept for 15 minutes, and starting a new export keeps
  only the most recent settled job.
- Visual and source exports wait for the capture service's start-up warm-up, capture the
  references, then produce their output. Source ZIP adds the portable viewer;
  browser-only ZIP skips capture. PDF merges renderer PDFs in capture-plan order.

## Export request and output

### Connection loss and cancellation

Jobs are owned by the running space server, not by the initiating HTTP connection.
The registry starts background work; closing a progress surface or disconnecting
does not cancel it. A client can query a known job ID while that server retains
it. Settled retention starts after completion, including for a slow job.

There is no public cancel operation or durable job-resume protocol. Server
restart loses the in-memory job registry; a former job ID cannot resume the
operation on the replacement server. Worker cleanup remains owned by the export
pipeline and host capture pool. Do not describe connection loss as successful
cancellation or automatic retry of an interrupted export.

Overall job deadlines and user-requested cancellation remain unspecified;
per-reference capture timeouts do not establish a maximum total export duration.
Their design must cover joined callers, worker disposal, partial artifacts and
retry identity before a cancel/recovery guarantee is added. The
[job tests](../../server/src/export/jobs.rs) cover settlement and retention;
disconnect/restart/cancel acceptance is not established by them.

### Options and output formats

`POST` accepts JSON: `format` (`zip`, `pdf`, `images`, `browser`), `scope`
(`space`, `page`, `pages`, `collection`), page IDs, collection name, `variants`
(`all`, `current`, `custom`), optional scenario/size filters, and lens. Current-view
requests include the selected page, scenario, declared artboard, iteration, lens,
size and viewport dimensions. Under an implementation lens the request drops the
iteration, so it does not enter the request or its job key: an implementation
shows no iteration. Selected references are deduplicated by page, scenario,
artboard, iteration, size and variant. Unknown pages, invalid options and mismatched current selections answer `400`.
Empty requests and direct GET downloads default to a whole-space source ZIP.

The dialog offers all declared variants or the current view. Source ZIP defaults
to all; single-page PDF/images default to current. Browser ZIP uses declared
variants. The request is recorded in ZIP metadata.

The capture helper renders each PDF page through its `printExportPage` command.
It settles the page, measures its document height, switches to screen media
for visual pages or print media for docs, and returns PDF bytes. Visual pages
use their viewport width and full document height, a custom paper size and
zero margins. Docs use A4/Letter, half-inch margins and automatic pagination.
Inserted print styles and media emulation are restored after every request.
Export owns selection, job progress, PDF assembly and downloading. The export
module merges the pages with `lopdf`,
preserving each page's size and selectable text. Individual capture failures are
warnings; no successful pages fails the job. PDF returns `application/pdf`.
Visual pages above 19,200 CSS pixels fail their capture rather than clip. Images
above 8,192 pixels likewise warn rather than silently truncate.

The portable build receives the selected page IDs and optional scenario/size
filters. Browser-only output fails if no compiled previews or docs are available.

- A failed portable build never fails a source ZIP. If the
  preview compiler cannot produce it at all, the export records a warning, leaves
  out `browser/`, and still delivers the sources and references. The warning
  retains the cause under `warnings` in `canonic-export.json`; individual preview
  build warnings remain under `browser.warnings` when a viewer is produced.
- The workbench shows modal progress ("Captured N of M"), pauses speculative
  interactive preparation, and leaves the visible selection and preview
  untouched. When the download finishes, it reports the total number of export
  warnings: source resolution, capture and portable build warnings.

## Reference capture plan

- The plan covers every declared scenario of every authored design page (one
  implicit `default` scenario when it declares none) at the page's sizes, plus
  each scenario iteration at every size. For a TypeScript preview, a scenario
  without declared artboards is captured at the page's sizes, plus each of its
  iterations at those sizes; a scenario that declares artboards captures each
  declared artboard once at its authored width and height, plus one reference
  per iteration of that artboard. A declared artboard's reference records the
  artboard ID as `size` and its label as `sizeLabel`. Pages are planned once
  each, across collections and the iOS Simulator catalog.
- Reference labels read `<Scenario> · <Artboard> · <Iteration>` for a declared
  artboard (`<Scenario> · <Artboard>` without an iteration) and
  `<Scenario> · <Iteration>` for an implicit artboard or authored page
  (`<Scenario>` without one).
- An export with an implementation lens drops every iteration reference:
  [iterations](../pages/iterations.md) belong to the design.
- An export whose lens (the request's `lens`, or the current lens for
  current-view requests) is a Storybook implementation captures, for every
  page mapped to that lens, each story of its mapped title at each of the
  page's export sizes, from `<url>/iframe.html?id=<story>&viewMode=story`,
  in place of the page's design references.
- Each page is captured at the export sizes the
  [sizes contract](../canvas/sizes.md#design-system-export) resolves for it.
  Screenshot records carry the size key as `size` and its label as
  `sizeLabel`, plus optional `artboard` and `iteration` IDs. README headings
  read `<reference label> · <size label>` (*Default · Laptop*), with the
  dimensions on the line below the image; a declared artboard's heading is its
  reference label alone.
- Design pages load as designed with actions off; the first scenario loads
  without a `scenario` parameter, and an iteration adds `iteration=<id>`. Current-view or explicit lens requests use the
  page's declared URL/workbench mapping, or a Storybook lens's mapped stories.
  Native lenses do not support visual export.
- TypeScript preview scenarios load the preview's page with `?scenario=<id>`,
  adding `artboard=<id>` for a declared artboard and `iteration=<id>` for an
  iteration. A
  capture waits up to 8 seconds for the preview to report it has rendered; a
  preview render error fails that reference instead of capturing an error
  page.
- Implementation-only pages (iOS Simulator catalog devices) and pages whose design file is missing produce warnings rather
  than invented images.

## Capture scheduling versus the camera

The [multiple-artboard support API](../canvas/multiple-artboards.md) composes interactive
screenshots and handoffs from every supplied instance. This does not change this
catalog-wide design-system export into an export of the current arrangement.
On a mixed-space canvas, the export action targets the selected artboard's
owning space; handoff images are stored by the canvas host space. Reuse
capture services through the boundaries in the
[code plan](../canvas/artboard-system.md), preserving
this export's own reference plan, scheduling, warnings, and archive contract.

| | Interactive camera and handoff | Design-system export |
| --- | --- | --- |
| Target | Current visible selection, scroll, annotations, and live document state | Planned design scenarios, TypeScript preview scenarios, and, with a Storybook lens, mapped pages' stories at configured sizes |
| Preparation | Continuous live mirror into one warm view; flush on click | Direct background loads and settling for each planned reference |
| Renderer use | Single queued helper | Up to four workers, each page's or story's references on one; the existing warm service is one of them |
| Reuse | Reprepare the selected view | Keep all reference sizes of one URL on its worker; switch Storybook stories in a loaded `iframe.html` runtime when possible |
| Result | Browser JPEG download or saved handoff screenshot | JPEG references embedded in the archive |

Export calls the same capture service but has its own jobs and scheduling.
References for one page or story stay on one worker to reuse the loaded
document across reference sizes. Each export supplies a fresh revision for
local references, so another export loads updated sources. Within one export,
local scripts initialize once per scenario URL and subsequent sizes resize and
settle that document. Other pages and stories run in parallel on
extra workers that start cold, each with its own helper; they close when the
export ends. Storybook reuse inside a worker does not navigate the visible
iframe. Storybook references wait for the story to render (up to 8 seconds),
then for fonts, visible images, and a short quiet period. A failed capture adds
a `captureWarnings` entry and the remaining references continue. A failure to
close an extra worker is also a warning and does not discard captures.

## Archive contract

- Everything sits under one top-level directory named after the target space
  (`<name>-design-system/`). An included `workbench.yaml` follows the manifest
  rules under [space scope](#space-scope).
- Each page gets an agent-oriented README beside its primary entry point and
  JPEGs in a nearby `screenshots/` directory. Pages that share a directory
  get `<page>.README.md` and `screenshots/<page>/`. A page with no design
  or source entry point goes under `workbench-pages/`. The README points to its
  reference images.
- `canonic-export.json` lists files, dependencies, unresolved references
  (`warnings`), page records (`pages`), `captureWarnings`, `browser`, and
  `parts`. Each page record has a SHA-256 content hash over its sorted exported
  source paths and raw file contents, including a TypeScript preview's
  compiler-resolved files. Generated READMEs and metadata, exporter-added
  package files, reference JPEGs, and compiled `browser/` output do not affect
  that hash.
- When the export fits in one ZIP of at most 10,000,000 bytes, the download is
  `<name>-design-system.zip`. Otherwise the download is an outer
  `<name>-design-system-parts.zip` holding a short README and numbered ZIPs of
  at most 10,000,000 bytes each. Parts extract into the same top-level
  directory, repeat the top README and `canonic-export.json`, and each has a
  `canonic-export-part-NN.json` file inventory. The outer ZIP itself has no
  size limit.
- A single file that cannot fit in one part fails the export with an error
  naming it.

### Portable TypeScript previews

When the project has TypeScript previews, the archive contains a `browser/`
directory: an interactive viewer at `browser/index.html` over a versioned
catalog, `browser/workbench.json`, with each preview compiled under its own
directory. Served by any static HTTP server, it opens without Electron,
Workbench, or the project's packages, and offers search, scenarios, sizes,
resizable dimensions, shared input controls, actions, and docs. Direct preview
pages accept `?scenario=<id>`, `?artboard=<id>` and `?iteration=<id>`. A preview that fails to build is listed under
`browser.warnings` while successful previews remain. `canonic-export.json`
records the entry, catalog, and each preview's id, source, entry, and scenarios.
The [previews spec](../pages/previews.md) owns the compiler, adapters, viewer, and the
CLI `build` command that produces the same viewer without a server.

## Verification points

- [server.test.js](../../server/node/server.test.js) checks, through the server
  and a stand-in capture service, progress, waiting for warm-up, coalesced
  jobs, the current-page PDF, retention and expiry (`404`), bounded retained
  bundles, job and part downloads, and the direct ZIP download. It also checks
  that a server selects only its configured space's pages;
  [source.rs](../../server/src/export/source.rs) starts page records from the
  resolved view's pages and copies the manifest through the source-root
  ownership check.
- The export module's tests check the capture plan for scenarios, artboards,
  docs pages and a Storybook lens's stories, the four-worker pool, per-story
  worker reuse, cleanup warnings, revision reuse within a job and refresh
  across jobs, the source closure, linked packages, Storybook configuration,
  shared-directory guide and screenshot placement, split parts, PDF merging,
  portable fallback, successful partial builds and archive failures after a
  portable failure.
- [preview.test.js](../../server/node/preview.test.js) checks that a Workbench
  lens's export captures the managed preview's scenarios, that a browser export
  builds only the selected previews, scenarios and sizes, and that the portable
  build keeps successful previews beside a failed one.
  [preview-astro.test.js](../../server/node/preview-astro.test.js) checks Astro reference
  scenarios in portable output.
- [capture-page.test.js](../../ui/legacy/capture-page.test.js) checks that
  reference capture waits for a TypeScript preview to render and fails on a
  render error. [preview-runtime.test.cjs](../../ui/preview/preview-runtime.test.cjs) checks
  that missing scenarios and render failures block readiness.
- [electron-capture.test.js](../../capture/electron-capture.test.js),
  [capture-helper.test.js](../../capture/capture-helper.test.js), and
  [capture-scripts.test.js](../../capture/capture-scripts.test.js) check the export pool, in-place
  Storybook switching, and settling.
- Not covered by automated tests: the `409` answer for an incomplete job,
  the missing-design and Simulator warnings, the exclusion of `browser/`
  output from page hashes, and the multi-space acceptance criteria under
  [space scope](#space-scope) end to end.
