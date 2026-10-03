# Design-system export contract

The design-system export packages the whole workbench as one download: the
source closure of every screen, reference screenshots, a guide per screen,
and, when the project has [TypeScript previews](previews.md), a portable
`browser/` viewer. It is a server-backed operation available from the editor
or a hosted browser; a `file://` workbench cannot export. See
[server.js](../server.js), [export.js](../export.js), and the
[capture contract](capture.md) for the renderer it uses. The user-facing
description is [Design-system export](../docs/design-system-export.md).

## Source closure

- Export starts from every screen's entry points: the design page, its code
  pointers, each imported Storybook story's file and component, and each
  TypeScript preview's definition, source, and the local files its compiler
  resolves (including adapter and environment dependencies). It follows
  reachable local imports and assets and keeps project-relative paths.
  Sources outside the workspace go under `implementations/`.
- It includes the Storybook and package configuration the included sources
  need. Installed dependencies, build output, secrets, tests, and unrelated
  files are excluded. When the project has no root `package.json`, the export
  adds one listing the external packages the sources import. With previews
  and no `workbench-env.d.ts`, it adds one with the `@canonic/workbench`
  types.

## Job lifecycle

- `POST /_workbench/export` starts a background job and answers `202` with
  its status. `GET /_workbench/export?job=<id>` reports progress, warnings,
  and, once complete, the download and its parts.
  `&download=1` downloads the completed bundle and `&part=<n>` one numbered
  part; an incomplete job answers `409`, and an expired one `404`. A `GET`
  without a job, for scripted callers, starts or joins a job, waits, and
  downloads.
- Only one export runs at a time; a request during a running export joins
  it. A settled job is kept for 15 minutes, and starting a new export keeps
  only the most recent settled job.
- Export waits for the capture service's start-up warm-up, captures the
  references, then builds the portable preview viewer, then the archive.
- A failed portable build never fails the export (decided 2026-10-03). If the
  preview worker cannot produce it at all, the export records a warning, leaves
  out `browser/`, and still delivers the sources and references.
- The workbench shows modal progress ("Captured N of M"), pauses speculative
  interactive preparation, and leaves the visible selection and preview
  untouched. When the download finishes, it reports the number of warnings.

## Reference capture plan

- The plan covers every declared state of every authored design screen, each
  story of every imported Storybook title, and each state of every TypeScript
  preview, at the screen's viewports. Screens are planned once each, across
  sections and imported catalogs.
- Viewport sizes: `desktop` 1512 × 982, `mobile` 393 × 852, `fit`
  1440 × 900, and `responsive` both desktop and mobile. A screen without
  `viewports` uses fit, desktop, and mobile. Duplicate sizes are removed.
- Design screens load as designed with actions off; the first state loads
  without a `state` parameter. Lenses on a design screen are not captured.
- TypeScript preview states load the preview's page with `?state=<id>`. A
  capture waits up to 8 seconds for the preview to report it has rendered; a
  preview render error fails that reference instead of capturing an error
  screen.
- Implementation-only screens other than Storybook (iOS Simulator catalog
  devices) and screens whose design file is missing produce warnings rather
  than invented images.

## Capture scheduling versus the camera

| | Interactive camera and handoff | Design-system export |
| --- | --- | --- |
| Target | Current visible selection, scroll, annotations, and live document state | Planned design states, TypeScript preview states, and imported Storybook stories at configured sizes |
| Preparation | Continuous live mirror into one warm view; flush on click | Direct background loads and settling for each planned reference |
| Renderer use | Single queued helper | Up to four workers for distinct page/story groups; the existing warm service is one of them |
| Reuse | Reprepare the selected view | Keep all viewport sizes of one URL on its worker; switch Storybook stories in a loaded `iframe.html` runtime when possible |
| Result | Browser JPEG download or saved handoff screenshot | JPEG references embedded in the archive |

Export calls the same capture service but has its own jobs and scheduling.
References for one page or story stay on one worker to reuse the loaded
document across viewport sizes. Other groups run in parallel on extra workers
that start cold, each with its own helper; they close when
the export ends. Storybook reuse inside a worker does not navigate the visible
iframe. Storybook references wait for the story to render (up to 8 seconds),
then for fonts, visible images, and a short quiet period. A failed capture adds
a `captureWarnings` entry and the remaining references continue. A failure to
close an extra worker is also a warning and does not discard captures.

## Archive contract

- Everything sits under one top-level folder named after the workbench
  (`<name>-design-system/`), with `workbench.yaml` at its root.
- Each screen gets an agent-oriented README beside its primary entry point and
  JPEGs in a nearby `screenshots/` directory. Screens that share a directory
  get `<screen>.README.md` and `screenshots/<screen>/`. A screen with no design
  or source entry point goes under `screens/`. The README points to its
  reference images.
- `canonic-export.json` lists files, dependencies, unresolved references
  (`warnings`), screen records, `captureWarnings`, `browser`, and `parts`.
  Each screen record has a SHA-256 content hash over its sorted exported
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
folder: an interactive viewer at `browser/index.html` over a versioned
catalog, `browser/workbench.json`, with each preview compiled under its own
directory. Served by any static HTTP server, it opens without Electron,
Workbench, or the project's packages, and offers search, states, viewports,
resizable dimensions, shared input controls, actions, and docs. Direct preview
pages accept `?state=<id>`. A preview that fails to build is listed under
`browser.warnings` while successful previews remain. `canonic-export.json`
records the entry, catalog, and each preview's id, source, entry, and states.
The [previews spec](previews.md) owns the compiler, adapters, viewer, and the
CLI `build` command that produces the same viewer without a server.

## Current status

### Verified behavior

The contract above matches the code as of 2026-10-03, with the gaps below.

### Implementation gaps

- Local references (design pages and TypeScript previews) are grouped per URL
  on one worker, but the export sends no revision, so the capture surface
  reloads the page for every viewport size ([capture-page.js](../workbench/capture-page.js)
  `frameLoaded`). Only Storybook references reuse the loaded document.
- The browser's closing message says "N screenshot warning(s)", but the
  count also includes portable preview build warnings
  ([server.js](../server.js) `beginExport`).
- If the preview worker cannot produce the portable build at all, the whole
  export job fails, contrary to the job lifecycle above.

### Open questions

- Should local references reuse one load across viewport sizes, as the
  scheduling contract says, or is a fresh load per size intended so scripts
  that measure the viewport at load see the right size?
- Should the closing message separate screenshot warnings from preview build
  warnings?

## Verification points

- [server.test.js](../server.test.js) checks the capture plan for states and
  imported stories, the four-worker pool, per-story worker reuse, progress,
  waiting for warm-up, coalesced jobs, retention and expiry (`404`), bounded
  retained bundles, cleanup warnings, job and part downloads, and the direct
  ZIP download.
- [export.test.js](../export.test.js) checks the source closure, linked
  packages, Storybook configuration, screen hashes, shared-folder guide and
  screenshot placement, and the split parts bundle.
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
  [capture.test.js](../capture.test.js) check the export pool, in-place
  Storybook switching, and settling.
- Not covered by automated tests: the `409` answer for an incomplete job,
  the missing-design and Simulator warnings, and the exclusion of `browser/`
  output from screen hashes.
