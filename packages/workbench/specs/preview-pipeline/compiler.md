# Compiler

Part of the [preview pipeline](README.md). This spec covers the development
compiler that builds previews and docs lenses for the canvas. It also says
which jobs stay on esbuild. Format handling and the project plugin contract
are in [plugins](plugins.md); delivering the output is in [serving](serving.md).

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
- The bundled binding is named through `NAPI_RS_NATIVE_LIBRARY_PATH` only while
  Rspack loads ([rspack.cjs](../../preview/rspack.cjs)). Other napi-rs modules
  read the same variable: with it left set, Astro 7's compiler loaded Rspack's
  binding and every Astro render failed with `a is not a function`
  (found 2026-10-13).
- Each platform VSIX bundles the matching pinned Rspack native binding, as it
  does esbuild. The installed extension downloads no compiler, and projects
  install none. The darwin-x64 Rspack binding is 50.4 MB, against 11.7 MB for
  esbuild, measured by the compiler speed lab on 2026-10-11.

### What stays on esbuild

These keep esbuild, so the VSIX ships both compilers
([finding 11](../../labs/rspack-pipeline/README.md#findings)):

| Job | Why |
| --- | --- |
| Evaluating definitions and `workbench.config.ts` | A Node CommonJS bundle with packages external, built with only the `@canonic2/workbench` plugin. It runs no project plugins and needs no hot updates |
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

## Lazy compilation

Rspack's `lazyCompilation` builds an entry only when a document first asks
for it. The prototype tried it on 2026-10-12, and it is not reliable yet
([finding 9](../../labs/rspack-pipeline/README.md#findings)):

- Rspack's middleware expects a POST, and Workbench's server forwards only
  GET requests to the worker. The prototype used a copy of Rspack's client
  that asks with GET.
- Most previews compiled on first request in 0.2–0.3 s.
- The first preview's request stalled, a compile error did not show for a
  lazily built entry, and pages stopped loading after a config change.
- Whether lazy compilation avoids moved chunks (see [entries](#entries)) is
  not established.

The pipeline builds every entry at start. Lazy compilation becomes a
requirement only if stage 2 of the lab shows that building every entry is too
slow on its huge fixture (200 previews).

## Discoveries

- **2026-10-11, compiler speed lab.** On one Intel macOS host, Rspack 2.2.7 in
  watch mode rebuilt the 40-page fixture in about 140 ms after a page or
  shared edit, against about 450 ms for an esbuild context. Its cold split
  build was 21–27% faster than esbuild's, and its development output is about
  three times esbuild's size. On the 5-preview fixture it was 14–22 ms slower
  than esbuild in sets 3 and 4, but faster than the shipping compiler in every
  shape. The shipping compiler spends about 1.1 s of Workbench's own work
  around esbuild for each cold preview
  ([report](../../labs/compiler-speed/README.md#re-measurement-run-sets-3-and-4)).
- **2026-10-13, packaging.** The darwin-x64 VSIX built with the pipeline is
  167.6 MB, against 147.9 MB for Workbench 0.13.0. It carries the Rspack
  binding in `preview/bin/` (50.4 MB unpacked) and webpack (2.5 MB), which
  Vue Loader's Rspack plugin loads for its rule classes.
- **2026-10-12, Rspack pipeline lab, stage 1.** The prototype compiler ran
  every native format in two profiles, on Electron 44.2 (Node 24.20), with
  output in memory and the persistent cache in the run's temporary folder.
  The project tree hashed the same before and after each run
  ([report](../../labs/rspack-pipeline/README.md#stage-1-report)).

## Open questions

- **The first page after a restart.** With the persistent cache warm, every
  entry rebuilds before the first page can show: 3.3–3.4 s on the 40-page
  fixture against 1.5–1.8 s for the shipping compiler, in stage 2. Lazy
  compilation, or another way to serve the first page early that keeps the
  entry set fixed for open documents, is needed to meet the first-open
  target.

- **Changing the entry set.** A preview or docs lens added or removed while
  pages are open changes the entry set. The proposed rule treats this like a
  configuration change: a new generation, with open documents reloading into
  a fresh warm host. Stage 1 did not cover it.
- **Bounds.** In stage 2 the persistent cache held 142 MB for 40 pages and
  389 MB for 200, and the worker peaked at 1.1–2.0 GB. Both need the bounds
  [performance](performance.md#resource-bounds) asks for. The 200-page start
  cells varied by up to two times between run sets and decide nothing.
