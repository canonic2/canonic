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

A selection mounts into it, so only the page's own chunks load. Two forms
were prototyped; stage 2 of the lab chooses one
([open questions](#open-questions)):

- **Warm spare frame.** A prepared spare frame waits with the runtime loaded.
  A selection takes the spare, and the next spare starts preparing. Each
  preview still gets its own document, which keeps isolation as it is today.
  Shared chunks and fonts come from the browser's HTTP and code caches.
- **Warm document.** One long-lived document per project and profile changes
  page in place, like an application's router. Between pages it restores its
  baseline (see [isolation](#isolation-in-a-reused-document)) and loads the
  next preview's project modules fresh, keeping dependencies, stylesheets and
  fonts in memory.

The canvas's existing warm host already reuses documents between previews
([previews](../previews.md#open-states-controls-and-actions)), so both forms
need the isolation rules below.

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
  Workbench's own globals. Removing them broke hot updates in stage 1
  ([finding 8](../../labs/rspack-pipeline/README.md#findings)).

Import-time body nodes keep their retention rule in
[previews](../previews.md#open-states-controls-and-actions).

### Resource bounds

- One warm runtime per project and profile: at most one prepared spare frame,
  or one warm document.
- Retained frames keep the limits in [core](../core.md#canvas-and-artboard): one
  active and up to three inactive sessions for 15 minutes. A warm document
  counts as one session.
- The heap of a warm document after a long walk, and of each retained frame,
  is measured in stage 2. If it grows with the number of pages shown, the
  document is replaced after a bound that stage 2 sets.

## Status

Implemented: pre-warming when the server starts the worker, and the warm
document. A new state, input or size of the preview a warm document shows
re-renders through the preview runtime without reloading its modules. Gaps:

- Returning to a page the warm document showed earlier loads its modules
  again; there are no other retained frames to resume. In the prototype this
  took 94–114 ms, against the proposed 100 ms target.
- The warm document is not replaced after a bound; its heap after long walks
  is not limited.

## Discoveries

- **2026-10-12, stage 1.** Both forms rendered every page and docs lens of the
  formats fixture across two walks in opposite orders. The fixture's
  isolation sentinels found nothing left behind on any page. Each page leaves
  a window listener, a timer, a portal node, a `<body>` class, a `window`
  global and a request mock on purpose
  ([report](../../labs/rspack-pipeline/README.md#stage-1-report)).
- **2026-10-13, stage 2.**
  - **Warm document:** new pages loaded about 0.36 MB in 2 scripts,
    against about 5 MB for the shipping pipeline, and showed in 0.14–0.18 s.
    The canvas heap after 26 new pages was 138–162 MB, about the shipping
    pipeline's.
  - **Warm spare:** new pages loaded 0.42 MB in 12 scripts and showed in
    0.23–0.37 s, and the canvas heap held about 270–300 MB.
  - **Pre-warming:** with pre-warming finished, the first page showed in
    0.81–0.83 s against 2.4–2.7 s without it
    ([report](../../labs/rspack-pipeline/README.md#stage-2-report)).
- **2026-10-12, readiness probe.** On a warm switch to a page that loads no
  web fonts, about 300 ms of the 600 ms from selection to ready went to
  loading the page's 5.2 MB bundle, which is mostly code shared with other
  pages ([lab evidence](../../labs/rspack-pipeline/README.md#evidence-so-far)).

## Open questions

- **Adopting the warm document.** The lab's rule chooses the warm document if
  it passes every isolation check and is at least 10% faster than the warm
  spare at the median, walking ten new pages on a large fixture. In stage 2
  it passed every check and was 24–55% faster in both run sets. It still
  re-evaluates modules for state, input and size changes and on return, which
  must re-render in place first. The owner decides.
- **Which page to prepare.** Preparing the most recently selected page's
  chunks, beyond the shared ones, is not specified yet.
