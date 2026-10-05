# Design-system export contract

The design-system export packages the whole current space as one download: the
source closure of every page in that space, reference screenshots, a guide per page,
and, when the project has [TypeScript previews](previews.md), a portable
`browser/` viewer. It is a server-backed operation available from the editor
or a hosted browser; a `file://` workbench cannot export. See
[server.js](../server.js), [export module](../src/modules/export/index.ts), and the
[capture contract](capture.md) for the renderer it uses. The user-facing
description is [Design-system export](../docs/design-system-export.md).

## Module ownership

The source and archive core lives under `src/modules/export/`, following the
[module layout contract](modules.md). Its public Node API is `index.ts`, with
typed inputs and reports in `types.ts`, filesystem-backed source closure and
page guides in `source.ts`, reference parsing in `references.ts`, and ZIP
assembly and partitioning in `archive.ts`. `build.ts` coordinates the optional
portable build and source archive, preserving a portable failure as a warning.
Tests live beside the module.

The server owns job lifecycle, capture planning and coordination with the capture
and preview services. Those services and shared host transport retain their
existing owners.

## Space scope

- One export targets one space, including all its collections, authored
  pages, discovered previews and docs pages, and imported catalog pages.
  It is independent of the selected page, collection, or lens. It does not
  aggregate other spaces listed in the switcher or configuration file.
- The export endpoint uses its server's selected configuration directory,
  space key, and root. It resolves that space's configuration and catalogs,
  then passes that resolved view to the source exporter, reference planner,
  and portable preview builder. It does not traverse the space registry.
- On a mixed-space canvas, the export action targets the selected artboard's
  owning space, as described under [capture scheduling](#capture-scheduling-versus-the-camera).
  The export still covers that entire space rather than the visible artboards.
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
- Selecting another page, collection, or lens within A leaves the export's
  space scope unchanged.
- Exporting B separately produces B's page records and reference plan.
- An included multi-space manifest retains its declarations without causing
  B's pages to enter A's export. Shared dependencies needed by A remain included.

## Source closure

Portable browser viewers include `browser/CANONIC-LICENSE.txt` for Canonic-owned
viewer code and generated authoring types. Project sources and third-party
components retain their own licensing.

- Export starts from every page's entry points in the target space: the design page, its code
  pointers, each imported Storybook story's file and component, and each
  TypeScript preview's definition, source, and the local files its compiler
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
- Only one export runs at a time per space server; a request to that server during a running export joins
  it. A settled job is kept for 15 minutes, and starting a new export keeps
  only the most recent settled job.
- Export waits for the capture service's start-up warm-up, captures the
  references, then builds the portable preview viewer, then the archive.
- A failed portable build never fails the export (decided 2026-10-03). If the
  preview worker cannot produce it at all, the export records a warning, leaves
  out `browser/`, and still delivers the sources and references. The warning
  retains the cause under `warnings` in `canonic-export.json`; individual preview
  build warnings remain under `browser.warnings` when a viewer is produced.
- The workbench shows modal progress ("Captured N of M"), pauses speculative
  interactive preparation, and leaves the visible selection and preview
  untouched. When the download finishes, it reports the total number of export
  warnings: source resolution, capture and portable build warnings.

## Reference capture plan

- The plan covers every declared state of every authored design page, each
  story of every imported Storybook title, and each state of every TypeScript
  preview, at the page's sizes. Pages are planned once each, across
  collections and imported catalogs.
- Capture each size the page supports, as resolved by its space. Fixed sizes
  use their dimensions; filled axes use 1440 wide or 900 tall. Fit uses
  1440 × 900. Resizable is skipped; a page whose only size is Resizable gets
  one 1440 × 900 reference. A page without `sizes` uses its space's sizes.
  Duplicate dimensions are removed. Screenshot records carry the size key
  as `size` and its label as `sizeLabel`; README headings read `Label · Laptop`.
  See the [sizes contract](sizes.md#design-system-export).
- Design pages load as designed with actions off; the first state loads
  without a `state` parameter. Lenses on a design page are not captured.
- TypeScript preview states load the preview's page with `?state=<id>`. A
  capture waits up to 8 seconds for the preview to report it has rendered; a
  preview render error fails that reference instead of capturing an error
  page.
- Implementation-only pages other than Storybook (iOS Simulator catalog
  devices) and pages whose design file is missing produce warnings rather
  than invented images.

## Capture scheduling versus the camera

The [multiple-artboard canvas](multiple-artboards.md) changes interactive
screenshots and handoffs to include the entire canvas. It does not change this
catalog-wide design-system export into an export of the current arrangement.
On a mixed-space canvas, the export action targets the selected artboard's
owning space; handoff images are stored by the canvas host space. Reuse
capture services through the boundaries in the
[code plan](multiple-artboards-code-plan.md), preserving
this export's own reference plan, scheduling, warnings, and archive contract.

| | Interactive camera and handoff | Design-system export |
| --- | --- | --- |
| Target | Current visible selection, scroll, annotations, and live document state | Planned design states, TypeScript preview states, and imported Storybook stories at configured sizes |
| Preparation | Continuous live mirror into one warm view; flush on click | Direct background loads and settling for each planned reference |
| Renderer use | Single queued helper | Up to four workers, each page's or story's references on one; the existing warm service is one of them |
| Reuse | Reprepare the selected view | Keep all reference sizes of one URL on its worker; switch Storybook stories in a loaded `iframe.html` runtime when possible |
| Result | Browser JPEG download or saved handoff screenshot | JPEG references embedded in the archive |

Export calls the same capture service but has its own jobs and scheduling.
References for one page or story stay on one worker to reuse the loaded
document across reference sizes. Each export supplies a fresh revision for
local references, so another export loads updated sources. Within one export,
local scripts initialize once per state URL and subsequent sizes resize and
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
Workbench, or the project's packages, and offers search, states, sizes,
resizable dimensions, shared input controls, actions, and docs. Direct preview
pages accept `?state=<id>`. A preview that fails to build is listed under
`browser.warnings` while successful previews remain. `canonic-export.json`
records the entry, catalog, and each preview's id, source, entry, and states.
The [previews spec](previews.md) owns the compiler, adapters, viewer, and the
CLI `build` command that produces the same viewer without a server.

## Current status

### Verified behavior

Portable-build fallback and local capture reuse are covered by module and server
regressions. Source, capture and portable warnings all contribute to the final
export warning count.

Space scope was confirmed by code inspection on 2026-10-04:
[server.js](../server.js) reads `config.read(where)` for its selected space,
and its export endpoint passes that resolved view into `beginExport`.
[source.ts](../src/modules/export/source.ts) starts page records from `view.pages` and copies
the manifest through the source-root ownership check. Existing
[server tests](../server.test.js) verify that a server selects only its
configured space's pages. The full multi-space export acceptance criteria
above do not yet have a dedicated end-to-end test.

## Verification points

- [server.test.js](../server.test.js) checks the capture plan for states and
  imported stories, the four-worker pool, per-story worker reuse, progress,
  waiting for warm-up, coalesced jobs, retention and expiry (`404`), bounded
  retained bundles, cleanup warnings, job and part downloads, and the direct
  ZIP download.
- [export tests](../src/modules/export/export.test.js) check the source closure, linked
  packages, Storybook configuration, page hashes, shared-directory guide and
  screenshot placement, and the split parts bundle. They also verify portable
  fallback, successful partial builds and archive failures after a portable
  failure. Server tests exercise a failed preview worker through job completion
  and ZIP download, and verify revision reuse within a job and refresh across jobs.
- [preview.test.js](../preview.test.js) checks that preview states enter the
  capture plan, that a preview's compiler-resolved sources are hashed and its
  `browser/` files archived, and that the portable build keeps successful
  previews beside a failed one.
  [preview-astro.test.js](../preview-astro.test.js) checks Astro reference
  states in portable output.
- [capture-page.test.js](../workbench/capture-page.test.js) checks that
  reference capture waits for a TypeScript preview to render and fails on a
  render error. [preview-runtime.test.js](../preview-runtime.test.js) checks
  that missing states and render failures block readiness.
- [electron-capture.test.js](../electron-capture.test.js),
  [capture-helper.test.js](../capture-helper.test.js), and
  [capture-scripts.test.js](../capture-scripts.test.js) check the export pool, in-place
  Storybook switching, and settling.
- Not covered by automated tests: the `409` answer for an incomplete job,
  the missing-design and Simulator warnings, and the exclusion of `browser/`
  output from page hashes.
