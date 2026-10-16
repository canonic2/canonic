# Warm runtime

Part of the [preview pipeline](README.md). This spec covers the work done
before a click so that the next page is quick: pre-warming when a project
starts, and keeping a runtime that has already loaded the shared code,
stylesheets and fonts. Frame retention and eviction are owned by
[core](../core.md#canvas-and-artboard); this spec adds what the pipeline keeps warm.

## Purpose

In an application with client-side navigation, the first page pays for
loading the framework, shared code, global stylesheets and fonts, and later
pages pay only for their own code. Previews should work the same way. A newly
selected page costs only its own chunks, its mount, and any font or image it
loads that the runtime does not have yet.

## Requirements

### Pre-warming on project start

- When a trusted space with native pages starts, the compile worker starts
  without waiting for a request. It indexes the project, declares every entry
  ([compiler](compiler.md#entries)), restores the persistent cache and builds.
- A request during pre-warming waits for that build, which contains its
  entry, and never for a later one.
- The canvas prepares its warm runtime as soon as the build reports, before a
  native page is selected.
- Pre-warming never runs project code outside the compile worker, and never
  runs in an untrusted space or with `previews: false`.

### The warm runtime

The canvas keeps, per project and profile, a runtime that has loaded:

- the preview runtime and request mocks;
- the profile's runtime chunk and the shared chunks;
- global stylesheets from the project environment;
- the web fonts those stylesheets load.

A selection mounts into one long-lived warm document per project and profile.
Between pages it restores its baseline and loads the selected preview's project
modules while retaining shared dependencies, stylesheets and fonts. State,
input and size changes re-render the current page without reloading its modules.
[Managed-preview lifecycle](../previews.md#open-states-controls-and-actions)
defines host mounting and retained import-time resources.

### Isolation in a reused document

A preview must leave nothing behind for the next one
([performance](performance.md#speed-never-costs-correctness)). Between
previews, a reused document removes everything the previous preview added
after the document's baseline:

- stylesheets, including those `style-loader` inserted;
- window and document listeners;
- timers, intervals and animation frames;
- body children outside the preview root, including portals;
- attributes and classes on `<html>` and `<body>`;
- `window` globals;
- request mock handlers.

It also:

- removes the previous preview's project modules from the module cache, so
  they evaluate fresh, and keeps modules from `node_modules`. `style-loader`'s
  own modules are evaluated fresh too, so its registry starts over;
- keeps the globals that the bundler and hot runtime own: the chunk registry,
  hot-update callbacks, React Refresh and Vue hot-reload hooks, and
  Workbench's own globals. Those globals belong to the retained compiler runtime, not to a page.

Import-time body nodes keep their retention rule in
[previews](../previews.md#open-states-controls-and-actions).

### Resource bounds

- At most one warm document per project and profile.
- Retained frames keep the limits in [core](../core.md#canvas-and-artboard): one
  active and up to three inactive sessions for 15 minutes. A warm document
  counts as one session.
- Warm-document and retained-frame memory must have measured bounds. A
  document whose retained resources exceed its bound is replaced without
  losing the requested view. Choosing the threshold and safe replacement
  behavior remains implementation work.

## Status

Implemented: pre-warming when the server starts the worker, and the warm
document. A new state, input or size of the preview a warm document shows
re-renders through the preview runtime without reloading its modules. Gaps:

- Returning to a page the warm document showed earlier loads its modules
  again rather than guaranteeing per-page runtime-state retention. The
  [retained-page requirement](performance.md#navigation-keeps-the-canvas-usable)
  and resource budget must be met together.
- The warm document is not replaced after a bound; its heap after long walks
  is not limited.

## Verification

Isolation and warm-host lifecycle are covered by
[preview-host tests](../../ui/legacy/preview-host.test.js) and
[preview-runtime tests](../../ui/preview/preview-runtime.test.cjs). The
[pipeline lab](../../labs/rspack-pipeline/README.md#stage-2-report) holds design
comparisons. Long-session heap bounds and real-host confirmation remain
acceptance work, not properties established by those prototype runs.

## Open questions

- **Which page to prepare.** Preparing the most recently selected page's
  chunks, beyond the shared ones, is not specified yet.
