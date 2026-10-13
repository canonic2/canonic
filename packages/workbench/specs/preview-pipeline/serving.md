# Serving

Part of the [preview pipeline](README.md). This spec covers how the compile
worker delivers compiler output to preview frames: URLs, caching, the preview
module that each preview address loads, and which routes skip the compile
queue.

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
  `data-rspack` attribute naming the chunk. With content-hashed names it
  fetched the old file
  ([finding 5](../../labs/rspack-pipeline/README.md#findings)).
- **Hot-update manifests and chunks** are served without caching.
- Asset files emitted by the compiler, such as images and fonts imported by
  sources, are served from the same prefix with content-hashed names.

### The preview module

A preview's development address returns a small ES module rather than the
entry chunk itself. It:

1. Imports the preview runtime and request mocks before any project module,
   as compiled previews do today ([previews](../previews.md#data-request-mocks-and-the-project-environment)).
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

- Output files, the event stream and preview modules bypass the worker's
  serial request queue. A long compile or export never delays a chunk that a
  document is waiting for.
- A failure while serving a file is logged and answered with an error status;
  it does not affect the queue.
- Preview and docs descriptors wait for the build that contains their entry
  ([compiler](compiler.md#entries)), not for the serial queue.

## Status

Implemented, with one gap: preview and docs descriptors are still answered in
the worker's serial queue, so a long export or Astro input render delays them.

## Discoveries

- **2026-10-12, stage 1.** The prototype served every format with these rules.
  Variant 3 kept one document per profile across every page of two walks of
  the formats fixture, loading each shared chunk once. CSS hot updates
  replaced the stylesheet in place for global CSS, CSS modules, Vue scoped
  styles and an HTML preview's linked stylesheet
  ([report](../../labs/rspack-pipeline/README.md#stage-1-report)).
- **2026-10-11, preview loading lab.** On its 40-page fixture, a project-wide
  shared build cut showing a new preview from 1.4 s to 0.13–0.2 s
  ([report](../../labs/preview-loading/README.md#results-large-fixture)).
