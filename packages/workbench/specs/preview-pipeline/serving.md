# Serving

Part of the [preview pipeline](README.md). This spec covers how the Workbench
server delivers compiler output to preview frames: URLs, caching, the preview
module that each preview address loads, and which routes skip the compile
queue.

This pipeline compiles/serves the project's previewed content. Generic UI asset
hosting is separate from this project pipeline and from Workbench UI composition;
artboards follow [content-only rendering](../rendering-flow.md). UI assets follow the required
[UI/backend independence contract](../modules.md#ui-and-backend-independence).

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
   ([previews](../previews.md#data-request-mocks-and-the-project-environment)).
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

### Routes

- Output files, the event stream and preview modules never wait behind
  other work, such as a long compile, an export or an Astro render. A chunk
  that a document is waiting for is answered as soon as it exists.
- A failure while serving a file is logged and answered with an error status;
  it does not affect the queue.
- Preview and docs descriptors wait for the build that contains their entry
  ([compiler](compiler.md#entries)), and for nothing else.

## Status

Implemented in the Node preview worker, which the Workbench server forwards
`/_workbench/previews/` to over a loopback TCP port until
[phase 4](../server.md#phase-4-previews). The worker answers most requests in
a serial queue; output files, the event stream and preview modules bypass it.
One gap: preview and docs descriptors are still answered in that queue, so a
long export or Astro input render delays them.

## Verification

[Pipeline tests](../../server/node/src/modules/preview-pipeline/pipeline.test.ts) cover
compiler output and serving. Shared-runtime isolation and stylesheet updates
also have [pipeline lab evidence](../../labs/rspack-pipeline/README.md#stage-2-report).
The remaining descriptor-queue gap must be closed without weakening build
error reporting or generation checks.
