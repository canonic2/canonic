# Warm runtime

Part of the [preview pipeline](README.md). This spec covers the work done
before a click so that the next page is quick: pre-warming when a project
starts, and keeping a runtime that has already loaded the shared code,
stylesheets and fonts. It also owns the budget of
[retained content sessions](#retained-sessions) that keep recent pages live.
The [Docs host](docs-host.md) owns the Docs pool built on that budget.

**Status:** Implemented: pre-warming in the server's preview compiler once the
page tree configures it, at startup when the host passes `--eager-previews`;
the warm preview document ([preview-host.js](../../ui/src/app/preview-host/preview-host.js),
served as `/_workbench/preview-host.html`), which re-renders scenario, input
and size changes without reloading modules and isolates successive previews;
and the shared retained-session budget. Not implemented: foreground priority
over warming, a browser runtime prepared before any native selection,
per-page runtime-state retention on return, and a measured bound that replaces
the warm document ([implementation gaps](#implementation-gaps)). The Docs pool's status is in the
[Docs host](docs-host.md).

## Purpose

In an application with client-side navigation, the first page pays for
loading the framework, shared code, global stylesheets and fonts, and later
pages pay only for their own code. Previews should work the same way. A newly
selected page costs only its own chunks, its mount, and any font or image it
loads that the runtime does not have yet.

## Requirements

Warm documents contain [project content only](../system/rendering-flow.md). Retaining a
full Workbench application document is not an acceptable warm-runtime mechanism.
Keep the existing Storybook channel and project content alive without retaining
hidden application controls or another canvas.

### Pre-warming on project start

- The host decides whether to spend resources warming a space: it passes
  `--eager-previews` to the server. With the flag, when a trusted space with
  native pages starts, the Workbench server's compiler starts without waiting
  for a request. It indexes the project, declares its entries
  ([compiler](compiler.md#entries)), restores the persistent cache and builds.
  Without the flag, the server starts this work on the first page-tree request.
  The extension and the standalone command both pass the flag.
- Warming never blocks the page list or the first requested page. Foreground
  requests take priority over warming work: a page the canvas asks for is
  listed and built ahead of pages nobody has requested, and never waits for a
  later build.
- The canvas prepares its warm runtime as soon as the build reports, before a
  native page is selected.
- Pre-warming never runs project code outside the Node evaluator, and never
  runs in an untrusted space or with `previews: false`.

### The warm runtime

For previews, the canvas keeps, per project and profile, a runtime that has loaded:

- the preview runtime and request mocks;
- the profile's runtime chunk and the shared chunks;
- global stylesheets from the project environment;
- the web fonts those stylesheets load.

A selection mounts into one long-lived warm document per owning content renderer,
project and profile. Independent artboards own independent mutable documents;
they may share compiled chunks and catalog data, not a document or input state.
Between pages it restores its baseline and loads the selected preview's project
modules while retaining shared dependencies, stylesheets and fonts. Scenario,
input and size changes re-render the current page without reloading its modules.
[Managed-preview lifecycle](../pages/previews.md#open-scenarios-controls-and-actions)
defines host mounting and retained import-time resources.

### Isolation in a reused document

A preview must leave nothing behind for the next one
([performance](../system/performance.md#speed-never-costs-correctness)). Between
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
[previews](../pages/previews.md#open-scenarios-controls-and-actions).

### Retained sessions

This section owns the retained content-session budget for every content kind:
previews, Docs and Storybook.

- Retained content sessions, including previews and Docs, preserve their
  attached iframe across selections: one active session and up to three
  inactive sessions for 15 minutes after departure.
- Returning preserves form edits, scroll, menus, and application state.
- Inactive frames are inert, keep their layout dimensions, and cannot send
  navigation or keyboard commands to the active canvas. A loading replacement
  is protected until promoted or cancelled.
- Docs and previews use the same retention budget across lenses and content
  kinds, not a separate budget per host kind or lens. A warm document counts
  as one session. Exact retained Docs returns show immediately and
  [check revisions in the background](docs-host.md#resume-and-freshness); new
  Docs destinations may reuse an idle same-lens document under the
  [warm Docs pool](docs-host.md#warm-docs-pool).
- Oldest inactive sessions are evicted first; expiry, Reload, configuration
  refresh, and canvas closure release their resources. Managed hosts dispose
  adapters and tracked page work before removal.
- Returning to a managed preview checks its compiled source revision before
  reuse and remounts if it changed.
- Sessions are local to the open canvas; no destination page is predicted or
  preloaded. The pipeline adds a warm runtime of shared code per project, not a
  predicted page, and keeps retained frames current through
  [hot updates](hot-updates.md#frames-that-apply-updates).
- A Storybook lens is retained under this same policy, and a renderer parked
  while explicit artboards are open is kept for up to 15 minutes; parked and
  inactive renderers cannot select a visible artboard, send editor navigation
  messages, or take part in captures or published context. Story-specific
  return behavior is in [Storybook](../pages/storybook.md).

### Resource bounds

- At most one preview warm document per owning content renderer, project and profile.
  Independent artboards are distinct owners. This bound does not permit a global
  mutable document shared by multiple simultaneously visible artboards.
- Retained frames keep the [retained-session](#retained-sessions) limits. The
  Docs pool's own bounds and accepted heap cost are in the
  [Docs host](docs-host.md#resource-bounds).
- Warm-document and retained-frame memory must have measured bounds. A
  document whose retained resources exceed its bound is replaced without
  losing the requested view.

## Verification

Isolation and warm-host lifecycle are covered by
[preview-host tests](../../ui/src/app/preview-host/preview-host.test.cjs) and
[preview-runtime tests](../../ui/preview/preview-runtime.test.cjs). The
[pipeline research](../../research/rspack-pipeline.md#stage-2-2026-10-13) holds design
comparisons. Long-session heap bounds and real-host confirmation remain
acceptance work, not properties established by those prototype runs.
[Content-frame tests](../../ui/legacy/content-frames.test.cjs) and
[session tests](../../ui/legacy/preview-sessions.test.js) cover retained
resume, the shared budget, expiry, Reload and disposal. Docs pool coverage is
in the [Docs host](docs-host.md#verification).

## Implementation gaps

- Warming does not give way to foreground requests. With `--eager-previews`,
  `Space::warm_docs` assembles the page tree at startup, which indexes the
  project and configures the compilers, then requests each docs lens's bundle
  in turn ([server](../system/server.md#request-tracing)); the compiler
  declares and builds every entry, and a first requested page waits for that
  whole build ([first-page gap](../system/performance.md#implementation-gaps)).
- Server-side pre-warming does not prepare the canvas's shared browser runtime
  before a native page is selected.
- Returning to a page the warm document showed earlier loads its modules
  again rather than guaranteeing per-page runtime-state retention. The
  [retained-page requirement](../system/performance.md#navigation-keeps-the-canvas-usable)
  and resource budget must be met together.
- The warm document is not replaced after a bound; its heap after long walks
  is not limited, and no threshold or safe replacement behavior is chosen.

## Open questions

- **Which page to prepare.** Preparing the most recently selected page's
  chunks, beyond the shared ones, is not specified yet.
