# Compiler

Part of the [preview pipeline](README.md). This spec covers the development
compiler that builds previews and docs lenses for the canvas. It also says
which jobs stay on esbuild. Format handling and the project plugin contract
are in [plugins](plugins.md); delivering the output is in [serving](serving.md).
The [page index](page-index.md) owns server-side listing. The worker retains
its own index for compilation and pre-warming; the Rust page-index integration
does not replace this compiler with the lab's Rust compiler host.

## Requirements

### One compiler per project and profile

- The compile worker runs one Rspack compiler in watch mode per space root and
  **profile**. A profile is a set of resolution rules shared by adapters:
  - `web`: React, Vue, HTML, Astro client output and custom adapters;
  - `react-native-web`: `react-native` aliased to `react-native-web`, and
    `.web.*` extensions preferred.

  Opening another page, state, size or lens never starts another compiler.
- Each compiler builds one compilation that contains every entry of its
  profile. Code that entries share is split into shared chunks, with one
  runtime chunk per profile ([serving](serving.md#chunks-and-urls)).
- The compiler runs in development mode with hot module replacement, and
  React Refresh for React sources ([hot updates](hot-updates.md)).

### Entries

- Every discovered preview, and every docs page in each of its docs lenses,
  is an entry. All of them are declared before the first build, and all are
  built.
- The entry set stays fixed while documents that loaded its output are open.
  Rspack's hot updates do not follow a module that moves to another chunk.
  Adding an entry can move shared modules into a new split chunk. A document
  that loaded the old layout then receives updates only for a chunk it never
  loaded, and silently applies nothing
  ([stage 1, finding 4](../../labs/rspack-pipeline/README.md#findings)).
- Each entry is a generated module that imports:
  - the definition;
  - the adapter's runtime;
  - the preview's sources and styles;
  - the project and preview environments.

  It registers the preview's mount with the runtime, and accepts hot updates
  to its sources ([hot updates](hot-updates.md#how-an-entry-applies-an-update)).
  A docs entry registers its examples, adapter and environment the same way.
- Generated entries are virtual modules at a reserved path under the project
  root, so project resolution applies to them. They are never written to disk.
- A request for an entry waits for the first finished build that contains
  it, and never for a later one.

### Output and cache

- Output is written to an in-memory file system and served from it
  ([serving](serving.md)). The compiler writes nothing inside the project.
- Rspack's persistent cache lives in the system temporary folder, scoped to
  the project root. Rspack's default location is inside the project
  (`node_modules/.cache/rspack`), so the compiler always sets it
  ([finding 2](../../labs/rspack-pipeline/README.md#findings)). A server
  restart with unchanged inputs restores the cache. An unreadable or stale
  cache rebuilds from source, and a cache failure never prevents compilation.
- Rspack frees a compilation when the next build starts, and reading it
  afterwards throws. When each build finishes, the compiler copies what
  serving needs:
  - each entry's files;
  - the runtime files;
  - the CSS chunks and their IDs;
  - the build errors per entry.
- Live source maps are separate files, fetched by developer tools.

### Configuration changes

- A change to `workbench.config.ts` replaces the compilers, because aliases,
  defines, rules and plugins are compiler options. Edits to environment files
  are ordinary module edits ([hot updates](hot-updates.md#the-reload-rule)). The worker closes the old ones, starts a new
  **generation**, declares the entries again and builds.
- Each generation's output has its own URL prefix. Documents that loaded an
  older generation reload into a fresh warm host
  ([hot updates](hot-updates.md#configuration-changes)).
- Changes are applied one at a time; a change during a replacement waits for
  it. Build results from a replaced compiler are ignored.

### Build errors

- Each build's errors are grouped by the entries whose module graph contains
  the failing module.
- An error reaches only the documents of those entries. One broken preview
  does not stop hot updates in the others
  ([hot updates](hot-updates.md#errors-and-recovery)).

### Runtime and packaging

- The compiler runs in the compile worker, on the
  [bundled Electron runtime](../capture.md#bundled-electron-runtime) in Node
  mode where that runtime is used, otherwise on the host's Node. Project code
  never runs in the extension host.
- Rspack 2 is published as ES modules only. The CommonJS worker loads it
  through Node 24's `require` of ES modules.
- Scope `NAPI_RS_NATIVE_LIBRARY_PATH` to loading the bundled Rspack binding
  ([rspack.cjs](../../preview/rspack.cjs)). Other napi-rs modules share that
  environment key and must resolve their own bindings.
- Each platform VSIX bundles the matching pinned Rspack native binding, as it
  does esbuild. The installed extension downloads no compiler, and projects
  install none. Packaging includes both the Rspack binding and esbuild,
  whose separate evaluation responsibilities are listed below.

### What stays on esbuild

These keep esbuild, so the VSIX ships both compilers
([finding 11](../../labs/rspack-pipeline/README.md#findings)):

| Job | Why |
| --- | --- |
| Evaluating definitions and `workbench.config.ts` | A Node CommonJS bundle with packages external, built with only the `@canonic2/workbench` plugin. It runs no project plugins and needs no hot updates. Listing pages bundles every definition that needs evaluating in one build ([page listing](performance.md#page-listing)) |
| The sidebar's component bundle and the docs page script | Workbench's own code, bundled in memory ([webview components](../../src/server/webview-components.ts)) |

The portable build and the export's docs examples use an Rspack production
compiler with the same format handling and project plugins
([plugins](plugins.md#portable-output)).

## Status

Implemented in [pipeline.ts](../../src/modules/preview-pipeline/pipeline.ts)
and [portable.ts](../../src/modules/preview-pipeline/portable.ts). Gaps:

- Live builds have no source maps. The lab measured builds without them, and
  turning them on is not yet measured.
- The first page after a restart waits for every entry to rebuild
  ([open questions](#open-questions)).
- An entry added while pages are open changes the entry set
  ([open questions](#open-questions)).

## First-page design constraints

A different start must allow the requested page to build independently of the
catalog size while preserving stable module/chunk identity and hot updates.
Lazy compilation and incremental entry registration are candidates only when
new pages, build errors, configuration replacement and already-open documents
all meet their contracts. The implemented compiler declares and builds every
entry; no alternative start is adopted.

The [first-page lab](../../labs/first-page/README.md#report) and
[compiler-host lab](../../labs/instant-first-page/README.md#stage-2-plan) hold
candidate comparisons. Packaging requirements follow the
[extension process boundary](../vscode-extension.md#process-boundaries) and
[code-quality gates](../code-quality.md).

## Open questions

- **The first page after a restart.** The implemented eager start still
  prepares and builds every entry. A start that meets the
  [first-page target](performance.md#measured-interactions) without scaling
  with catalog size remains open.

- **Changing the entry set.** A preview or docs lens added or removed while
  pages are open changes the entry set. The proposed rule treats this like a
  configuration change: a new generation, with open documents reloading into
  a fresh warm host. Stage 1 did not cover it.
- **Bounds.** Compiler memory and persistent-cache storage need explicit
  limits and eviction rules. Measure long sessions and large projects before
  choosing them; page-index bounds do not bound compilation.
