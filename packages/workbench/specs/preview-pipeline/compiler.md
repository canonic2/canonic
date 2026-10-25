# Compiler

Part of the [preview pipeline](README.md). This spec covers the Workbench
server's development compiler that builds previews and docs lenses for the
canvas. It also says which jobs run in the Node evaluator. Format handling
and the project plugin contract are in [plugins](plugins.md); delivering the
output is in [serving](serving.md). The [page index](page-index.md) owns
server-side listing; the compiler builds what its listing found.

**Status:** Implemented in [compiler.rs](../../server/src/compiler.rs) and its
modules, with portable builds in [portable.rs](../../server/src/compiler/portable.rs):
one compiler per space and profile, generated entries, in-memory output, the
persistent cache, generations on configuration changes, per-entry build errors
and the Node evaluator jobs. The implemented start declares and builds every
entry before serving the first; that first-page gap is owned by
[performance](../system/performance.md#implementation-gaps), with a proposed replacement
start in [first page](first-page.md). Live source maps, a rule for
entries added while pages are open and compiler bounds are not implemented
([gaps](#gaps)).

## Requirements

### One compiler per project and profile

- The Workbench server runs one Rspack compiler in watch mode per space root and
  **profile**. A profile is a set of resolution rules shared by adapters:
  - `web`: React, Vue, HTML, Astro client output and custom adapters;
  - `react-native-web`: `react-native` aliased to `react-native-web`, and
    `.web.*` extensions preferred.

  Opening another page, scenario, size or lens never starts another compiler.
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
- Each page-tree request configures the compilers with the space's
  `previews` settings and the page index's listing, and they pre-warm every
  listed preview and docs lens after applying it. For `previews: false`, the
  compilers close; an untrusted workspace never configures them. A change to
  the resolved `previews` value replaces the compilers with new ones. A
  preview or docs request that arrives first assembles the page tree.

### Entries

- Every discovered preview, and every docs page in each of its docs lenses,
  is an entry.
- Only discovered definitions inside the project compile. A request for one
  definition checks discovery rules directly without scanning every project
  file.
- The entry set stays fixed while documents that loaded its output are open.
  Rspack's hot updates do not follow a module that moves to another chunk.
  Adding an entry can move shared modules into a new split chunk. A document
  that loaded the old layout then receives updates only for a chunk it never
  loaded, and silently applies nothing
  ([stage 1, finding 4](../../research/rspack-pipeline.md#stage-1-findings)).
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
  ([finding 2](../../research/rspack-pipeline.md#stage-1-findings)). A server
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
  Rspack's nightly toolchain ([server previews](../system/server.md#previews)).
  Where project code runs is the server's
  [process model](../system/server.md#processes-and-connections).
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
The server owns [the evaluator's process](../system/server.md#the-node-evaluator),
its lifetime and deadlines, and the page index's separate
[definition-index evaluator](../system/server.md#the-definition-index-evaluator).
The compiler sends the evaluator these jobs:

| Job | How |
| --- | --- |
| Evaluating definitions and `workbench.config.ts` | esbuild bundles each with packages external and only the `@canonic2/workbench` plugin, and Node runs it. A definition whose list fields are literals is read in Rust and needs no evaluation. The config returns as data |
| A project's JavaScript loaders | One module at a time, with the common parts of the webpack loader context |
| Vue single-file components | Workbench's bundled `@vue/compiler-sfc` returns the script and each style block ([Vue adapter](../adapters/vue.md#build)) |
| Astro rendering | The project's Astro through its container, with the scenario's request mocks ([Astro adapter](../adapters/astro.md#build)) |
| Workbench browser UI and docs page script | Not the evaluator: browser-native JavaScript/CSS loaded or copied unchanged |

The portable build and the export's docs examples use a production
compiler with the same format handling and project plugins
([plugins](plugins.md#portable-output)).

## Gaps

- Live builds have no source maps. The lab measured builds without them, and
  turning them on is not yet measured.
- An entry added while pages are open changes the entry set
  ([open questions](#open-questions)).
- The compiler's memory, and the persistent cache's growth over long
  sessions, are unmeasured ([open questions](#open-questions)).

## Decisions

- Rspack is the development compiler, with hot updates for every native
  format, shared chunks, pushed build results and a warm document. Portable
  output uses an Rspack production build with the same project plugin form.

## Open questions

- **Changing the entry set.** A preview or docs lens added or removed while
  pages are open changes the entry set. The proposed rule treats this like a
  configuration change: a new generation, with open documents reloading into
  a fresh warm host. Stage 1 did not cover it.
- **Bounds.** Compiler memory and persistent-cache storage need explicit
  limits and eviction rules. Measure long sessions and large projects before
  choosing them; page-index bounds do not bound compilation.
