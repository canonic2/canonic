# Serving

Part of the [preview pipeline](README.md). This spec covers how the Workbench
server delivers compiler output to preview frames: URLs, caching, the preview
module that each preview address loads, and which requests wait for a
build.

This pipeline compiles/serves the project's previewed content. Generic UI asset
hosting is separate from this project pipeline and from Workbench UI composition;
artboards follow [content-only rendering](../system/rendering-flow.md). UI assets follow the required
[UI/backend independence contract](../system/modules.md#ui-and-backend-independence).

**Status:** Implemented in `workbench-server`, which answers
`/_workbench/previews/` and `*.workbench.ts(x)` page addresses
([previews.rs](../../server/src/serve/previews.rs)) once its page tree has
configured previews ([server](../system/server.md#previews)): generation URL
prefixes and caching, the preview module, preload selection for preview and
Docs entries in the generated client, and concurrent routes with no serial
queue. The one gap, descriptors waiting for pre-warming, is listed in
[gaps](#gaps).

## Requirements

### Chunks and URLs

- All output of one compiler generation is served under one URL prefix that
  names the profile and generation ([compiler](compiler.md#configuration-changes)).
  A new generation never reuses an older one's URLs.
- Code shared by entries is split into shared chunks, with one runtime chunk
  per profile. A document loads each shared chunk once, however many previews
  it shows ([warm runtime](warm-runtime.md)).
- **Scripts** have content-hashed names and are served as immutable, so the
  browser's HTTP and code caches keep them across pages, frames and reloads.
  A preview module's list of scripts therefore always names files from one
  build.
- **Stylesheets** keep stable names per chunk and are served without caching.
  Rspack's CSS hot update fetches a chunk's new stylesheet by the name the
  document already knows. It finds the old link by URL prefix or by a
  `data-rspack` attribute naming the chunk, so updates address the current CSS.
- **Hot-update manifests and chunks** are served without caching.
- Asset files emitted by the compiler, such as images and fonts imported by
  sources, are served from the same prefix with content-hashed names.

### The preview module

A preview's development address returns a small ES module rather than the
entry chunk itself. It:

1. Imports the preview runtime and request mocks before any project module
   ([previews](../pages/previews.md#data-request-mocks-and-the-project-environment)).
2. Throws the entry's build error at once if its last build failed, so the
   frame shows it ([hot updates](hot-updates.md#errors-and-recovery)).
3. Adds the entry's stylesheets as links tagged with `data-rspack`, unless the
   document already has them.
4. If the document already runs this profile's runtime because it showed
   another preview, brings it to the current build hash through a hot update
   first. If that update fails, the frame reloads into a fresh warm host.
   It then removes project modules from the module cache, so the preview's
   own modules evaluate fresh, and keeps dependencies from `node_modules`
   ([warm runtime](warm-runtime.md#isolation-in-a-reused-document)).
5. Loads only the scripts the document does not have yet.
6. Mounts the registered entry.

The descriptor tells the runtime the preview is a development build that
receives pushed updates, so the runtime starts no revision poll
([hot updates](hot-updates.md#push)).

### Preload selection

Before inserting script preloads, the generated page module checks both the
document's loaded-script set and the initialized runtime for the requested
profile. A runtime chunk already supplied by that profile is neither preloaded
nor executed again, even if its current URL is absent from the loaded-script
set. Runtime scripts load on a cold document. Missing shared and entry
chunks preload concurrently with styles and execute in dependency order after
styles settle.

This applies to preview and Docs entries, including both warm hosts. It must
preserve runtime build synchronization, fresh evaluation of project modules,
stylesheet hot updates and cold-page startup. Focused Rust client-module tests
cover initialized and cold runtimes and load order. No automated test checks
warm Docs navigation for unused-runtime-preload warnings; the
[warm Docs research](../../research/docs-navigation.md#warm-docs-integration-2026-10-23)
records that observation.

### Routes

- Output files, the event stream and preview modules never wait behind
  other work, such as a long compile, an export or an Astro render. A chunk
  that a document is waiting for is answered as soon as it exists.
- A failure while serving a file is logged and answered with an error status;
  it does not affect other requests.
- Preview and docs descriptors wait for the build that contains their entry
  ([compiler](compiler.md#entries)), and for nothing else.

## Gaps

- Preview and docs descriptors wait for pre-warming to finish
  ([first-page gap](../system/performance.md#implementation-gaps)).

## Verification

[preview.test.js](../../server/node/preview.test.js) and
[preview-docs.test.js](../../server/node/preview-docs.test.js) cover
compiler output and serving through the server, and the compiler's Rust unit
tests cover entries, the generated client and HTML modules. Shared-runtime
isolation and stylesheet updates also have
[pipeline research](../../research/rspack-pipeline.md#stage-2-2026-10-13).
