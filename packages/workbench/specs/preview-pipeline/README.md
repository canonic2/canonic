# Preview pipeline

The preview pipeline is how Workbench turns a project's native pages
(TypeScript previews and docs) into something the canvas shows. It compiles
them, serves the output, loads it into a frame, mounts it, waits for it to be
ready, and applies edits. These specs define the target pipeline and how
Workbench gets there. [TypeScript previews](../previews.md) and
[docs](../docs-pages.md) still own what a preview or docs page is and does.
This folder owns how they are built, delivered and kept current.

## The pipeline

1. **Discover and list.** The native page index walks the project, parses
   literal definitions, and evaluates computed definitions and docs in a
   separate Node process. A saved index avoids re-reading unchanged inputs
   ([previews](../previews.md#author-and-discover)).
2. **Evaluate.** The compile worker evaluates definitions and
   `workbench.config.ts` in Node, bundled with esbuild. Every definition
   that needs evaluating is bundled in one build ([compiler](compiler.md#what-stays-on-esbuild)).
3. **Compile.** One Rspack development compiler per project and profile
   builds every preview and docs lens in one compilation, with shared chunks,
   hot updates and a persistent cache ([compiler](compiler.md)).
4. **Serve.** The worker serves the output from memory. Shared chunks get
   content-hashed URLs and are cached as immutable. Each preview gets a small
   preview module that loads only the chunks its document is missing
   ([serving](serving.md)).
5. **Load and mount.** A warm frame, or a warm document, that has already
   loaded the runtime and shared code mounts the preview ([warm runtime](warm-runtime.md)).
6. **Ready.** Readiness waits only for fonts and images that are still loading
   ([readiness](readiness.md)).
7. **Update.** The server pushes build results to the canvas. Open frames apply
   hot updates in place, or reload when an edit cannot apply
   ([hot updates](hot-updates.md)).

Project plugins and Workbench's own format handling are Rspack loaders and
plugins ([plugins](plugins.md)). The [performance](performance.md) spec owns
the targets the whole pipeline is measured against.

## Topics

| Spec | Covers |
| --- | --- |
| [Performance](performance.md) | Requirements for render, navigation and edits; proposed targets; Workbench versus project time; diagnostics; measured status |
| [Compiler](compiler.md) | Rspack compilers per project and profile, the fixed entry set, in-memory output, the persistent cache, config changes, build errors, the runtime and packaging, what stays on esbuild, lazy compilation |
| [Serving](serving.md) | Output URLs and caching, the preview module, stylesheet names, worker routes outside the compile queue |
| [Hot updates](hot-updates.md) | The push channel, edit behavior per format, the reload rule, state kept across edits, build errors and recovery, hidden frames |
| [Warm runtime](warm-runtime.md) | Pre-warming on project start, the warm spare frame and the warm document, isolation between previews in a reused document, resource bounds |
| [Readiness](readiness.md) | What readiness waits for, phase diagnostics, rendering hidden frames |
| [Plugins](plugins.md) | Workbench's format handling as Rspack loaders and plugins, and the project plugin contract in `workbench.config.ts` |

## Decisions

- **2026-10-11, owner:** native pages support hot module replacement, and
  React components keep their state through React Fast Refresh
  ([performance](performance.md#edits-apply-in-place)).
- **2026-10-11, owner:** a compiler change is justified by a 10% gain. This
  replaced the 30% threshold first written into the
  [compiler speed lab](../../labs/compiler-speed/README.md). Under it, Rspack
  meets the lab's large-fixture speed criterion: 21–27% faster than esbuild on
  cold builds and 26–71% faster on rebuilds. Of the candidates that can drive
  HMR with React Fast Refresh from code, Rspack has the fastest rebuilds.
- **2026-10-12, owner:** agreed the pipeline plan, in the form of the
  [Rspack pipeline lab's question](../../labs/rspack-pipeline/README.md#question):
  - Rspack as the development compiler;
  - hot updates for HTML, CSS, JavaScript, React, Vue, Astro and docs;
  - a warm frame or document for the next page;
  - pre-warming on project start;
  - project and Workbench plugins rewritten for Rspack;
  - pushed edits;
  - shared chunks;
  - readiness fixes.
- **2026-10-12, owner:** accepted stage 1 of the
  [Rspack pipeline lab](../../labs/rspack-pipeline/README.md#stage-1-report).
  A prototype of the whole pipeline passed every correctness check on a
  project with every native format. Accepting it adopted these changes to the
  plan:
  - the entry set is fixed and every entry is built at start, rather than
    compiled lazily ([compiler](compiler.md#entries));
  - hidden frames apply hot updates as they arrive
    ([hot updates](hot-updates.md#frames-that-apply-updates));
  - Vue style blocks use the classic CSS loaders ([plugins](plugins.md#workbench-format-handling)).
- **2026-10-13, owner:** implement the pipeline, on the evidence of
  [stage 2](../../labs/rspack-pipeline/README.md#stage-2-report). The font
  wait gets its own lab after implementation. With the decision:
  - the warm runtime is the **warm document**, which re-renders state,
    input and size changes in place, and returning to its page as well
    ([warm runtime](warm-runtime.md));
  - readiness keeps waiting on `document.fonts.ready` until that lab
    settles a faster rule ([readiness](readiness.md));
  - the first page after a restart keeps waiting for the full build. Showing
    it earlier stays open ([compiler](compiler.md#open-questions));
  - the portable build moves to an Rspack production build, so projects
    write one plugin form ([plugins](plugins.md#open-questions)).

- **2026-10-14, owner:** evaluate the page list's definitions in one esbuild
  build instead of one build per definition. An esbuild build costs tens of
  milliseconds however little it holds, which made listing 600 pages take
  7.4 s ([performance](performance.md#page-listing)).

- **2026-10-15, owner:** on the
  [first page lab](../../labs/first-page/README.md#report), which adopted no
  start:
  - every entry is still built at start. Adding entries in batches, and a
    cached page index, are not pursued: batches slowed every later build
    several times over, and the cached index saved little once listing was
    one build ([compiler](compiler.md#discoveries));
  - callers listing at the same time share one definitions build
    ([performance](performance.md#page-listing));
  - the first page's time must not grow with the number of pages, which the
    shipping start does not meet yet
    ([performance](performance.md#first-page)).
- **2026-10-15, owner:** the first page shows within 500 ms of the canvas
  opening, and the complete page list within 200 ms, at any page count, with
  a warm cache; with a cold cache, the first page shows in about its own
  build time ([performance](performance.md#measured-interactions)). Workbench
  moves to Rust where it can, as a VS Code extension and as part of Studio;
  Electron stays for screenshots.
- **2026-10-15, owner:** on stage 1 of the
  [instant first page lab](../../labs/instant-first-page/README.md#stage-1-report),
  implement its Rust page index as measured: a saved index checked by file
  and directory stamps, list fields parsed from literal definitions, and the
  shipping evaluation for the rest. The lab's stage 2, the Rust compiler host
  end to end, continues in parallel.

The other proposed targets in [performance](performance.md#measured-interactions)
are not agreed yet.

## Status

The Rust page index is implemented in `native/`, with the server adapter in
`src/modules/page-index/`, following the [2026-10-15 decision](#decisions).
It lists pages independently of the compile worker. The compiler host remains
the instant first page lab's stage 2 work; compilation and pre-warming keep
their existing index and start behavior.

The Rspack preview pipeline is implemented in [src/modules/preview-pipeline/](../../src/modules/preview-pipeline/)
and the preview runtime, canvas and docs page. Each topic's status names what
remains. The implementation passes the package's tests and the
[Rspack pipeline lab](../../labs/rspack-pipeline/README.md)'s stage 1 checks
on the formats fixture, run against the product code. The confirmation run the
[labs methodology](../labs.md#reports-and-decisions) asks for, the lab's
sequences through the shipping server and the VS Code webview, has not been
made yet.

The evidence for the design is the lab's
[stage 2](../../labs/rspack-pipeline/README.md#stage-2-report) (2026-10-13,
two run sets on the prototype): new pages and edits about 2 to 15 times faster
than the esbuild pipeline, within the proposed targets except the first page
after a restart and, in the prototype, the warm document's state changes and
returns.

## Open questions

- Whether Studio should consume `workbench-native` as a Rust library when
  the compiler host joins it. The package has one library and binary; no
  Studio dependency is established.
- Whether a resident index with a watcher can meet the page-list budget on
  cold 10,000-page starts. The shipped index spawns once per request and
  does not watch definitions.
- Whether untrusted workspaces should list literal definitions. They list
  none under the current trust contract.
- Whether the worker's pre-warming should use the native file list. It
  retains `Compiler.index()` pending stage 2.

Each topic lists its own. The ones that change the plan:

- Whether to adopt the warm document, which the lab's rule chose
  ([warm runtime](warm-runtime.md#open-questions)).
- How to show the first page without waiting for the whole page list,
  which the canvas loads before any page. The
  [first page lab](../../labs/first-page/README.md#report) found no start
  that meets its rule: on 600 pages the best first page took about three
  times as long as on 5, with preparing every entry and loading the page
  list still growing with the project.
- How to show the first page after a restart without waiting for every
  entry: lazy compilation, if it can be made reliable, or another way
  ([compiler](compiler.md#open-questions)).
- How readiness waits for fonts a page starts loading
  ([readiness](readiness.md#open-questions)).
- How the entry set changes when a preview or docs lens is added or removed
  while pages are open ([compiler](compiler.md#open-questions)).
