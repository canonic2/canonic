# Compiler

Part of the [preview pipeline](README.md). This spec covers the Workbench
server's development compiler that builds previews and docs lenses for the
canvas. It also says which jobs run in the Node evaluator. Format handling
and the project plugin contract are in [plugins](plugins.md); delivering the
output is in [serving](serving.md). The [page index](page-index.md) owns
server-side listing; the compiler builds what its listing found.

## Requirements

### One compiler per project and profile

- The Workbench server runs one Rspack compiler in watch mode per space root and
  **profile**. A profile is a set of resolution rules shared by adapters:
  - `web`: React, Vue, HTML, Astro client output and custom adapters;
  - `react-native-web`: `react-native` aliased to `react-native-web`, and
    `.web.*` extensions preferred.

  Opening another page, state, size or lens never starts another compiler.
- The server watches the space root recursively, and every folder outside it
  that holds a file a build, an Astro render or `workbench.config.ts` read,
  one level deep: a monorepo package linked through `node_modules` or imported
  by a relative path rebuilds when it changes. Folders inside `node_modules`
  and folders above the root are not watched; watching above the root would
  watch the root's whole tree.
  Dependency folders own independent watches, so registering or removing one
  cannot interrupt the root or an already-watched dependency. Shutdown and
  reconfiguration release the watches and their event consumers together.
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
- Rspack's cache trusts a file whose modification time matches its snapshot.
  Generated modules, which live only in memory, therefore get a new
  modification time whenever their content changes and on every server start.
  After a restart the cache compares their content, so a render changed
  while the server was stopped rebuilds and an unchanged one is reused.
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
  are ordinary module edits ([hot updates](hot-updates.md#the-reload-rule)). The server closes the old ones, starts a new
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

- The compiler is Rspack's Rust crates inside `workbench-server`, built with
  Rspack's nightly toolchain ([phase 4](../system/server.md#previews)).
  Server-side project evaluation runs in the Node evaluator, outside the native
  server and extension host. Project browser code runs in preview documents,
  including those opened by the capture helper
  ([processes and connections](../system/server.md#processes-and-connections)).
- The installed extension downloads no compiler, and projects install none.

The compiler is [compiler.rs](../../server/src/compiler.rs) in
`workbench-server`. Each profile's Rspack compiler belongs to
its own builder task, which applies entries, generated modules and file
changes in batches and keeps a snapshot of what serving needs from each
build. Rspack's Rust builder needs three things the JavaScript one supplies:
webpack's default `./src` entry is replaced by an empty module, the `web`
target's HTTP externals (remote URLs a stylesheet imports) are added, and
`splitChunks: { chunks: "all" }` is the split-chunks plugin with Rspack's
development defaults.

### What runs in the Node evaluator

Evaluating project code is the Node evaluator's job, not the compiler's.
The evaluator is one long-lived Node process per space
([node.rs](../../server/src/node.rs),
[evaluator/](../../server/node/src/modules/evaluator/index.ts)), started on
first need and reached over stdin and stdout with one JSON request per line:

This describes compiler evaluation. The page index currently uses a separate
short-lived evaluation/selection process; [server process ownership](../system/server.md#processes-and-connections)
records that deviation from the shared-evaluator target.

| Job | How |
| --- | --- |
| Evaluating definitions and `workbench.config.ts` | esbuild bundles each with packages external and only the `@canonic2/workbench` plugin, and Node runs it. A definition whose list fields are literals is read in Rust and needs no evaluation. The config returns as data |
| A project's JavaScript loaders | One module at a time, with the common parts of the webpack loader context |
| Vue single-file components | Workbench's bundled `@vue/compiler-sfc` returns the script and each style block ([Vue adapter](../adapters/vue.md#build)) |
| Astro rendering | The project's Astro through its container, with the state's request mocks ([Astro adapter](../adapters/astro.md#build)) |
| Workbench browser UI and docs page script | Not the evaluator: browser-native JavaScript/CSS loaded or copied unchanged |

The portable build and the export's docs examples use a production
compiler with the same format handling and project plugins
([plugins](plugins.md#portable-output)).

## Status

Implemented in [compiler.rs](../../server/src/compiler.rs) and its modules,
with portable builds in [portable.rs](../../server/src/compiler/portable.rs).
Gaps:

- Live builds have no source maps. The lab measured builds without them, and
  turning them on is not yet measured.
- The first page after a restart waits for every entry to build
  ([open questions](#open-questions)).
- An entry added while pages are open changes the entry set
  ([open questions](#open-questions)).
- The compiler's memory, and the persistent cache's growth over long
  sessions, are unmeasured ([open questions](#open-questions)).

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
[extension process boundary](../integrations/vscode-extension.md#process-boundaries) and
[code-quality gates](../system/code-quality.md).

## Open questions

- **The first page after a restart.** The implemented eager start still
  prepares and builds every entry. A start that meets the
  [first-page target](../system/performance.md#measured-interactions) without scaling
  with catalog size remains open.

- **Changing the entry set.** A preview or docs lens added or removed while
  pages are open changes the entry set. The proposed rule treats this like a
  configuration change: a new generation, with open documents reloading into
  a fresh warm host. Stage 1 did not cover it.
- **Bounds.** Compiler memory and persistent-cache storage need explicit
  limits and eviction rules. Measure long sessions and large projects before
  choosing them; page-index bounds do not bound compilation.
