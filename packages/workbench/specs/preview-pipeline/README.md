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
   ([page index](page-index.md)).
2. **Evaluate.** The Node evaluator runs definitions and
   `workbench.config.ts`, which are project code. Every definition that needs
   evaluating is bundled in one build ([compiler](compiler.md#what-runs-in-the-node-evaluator)).
3. **Compile.** The Workbench server's compiler, one Rspack development
   compiler per project and profile, builds every preview and docs lens in
   one compilation, with shared chunks, hot updates and a persistent cache
   ([compiler](compiler.md)). Adapters that need project code on the server,
   such as Astro renders, run that part in the Node evaluator
   ([adapters](../adapters/README.md#how-pages-are-served)).
4. **Serve.** The Workbench server serves the output from memory. Shared
   chunks get content-hashed URLs and are cached as immutable. Each preview
   gets a small preview module that loads only the chunks its document is
   missing ([serving](serving.md)).
5. **Load and mount.** A warm document that has already
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
| [Page index](page-index.md) | Complete ordered listing, definition parity, saved-state invalidation, process and storage bounds, failure recovery and acceptance evidence |
| [Performance](performance.md) | Requirements for render, navigation and edits; proposed targets; Workbench versus project time; diagnostics; measured status |
| [Compiler](compiler.md) | Rspack compilers per project and profile, the fixed entry set, in-memory output, persistent caching, config changes, build errors, packaging and first-page constraints |
| [Serving](serving.md) | Output URLs and caching, the preview module, stylesheet names, which requests wait for a build |
| [Hot updates](hot-updates.md) | The push channel, edit behavior per format, the reload rule, state kept across edits, build errors and recovery, hidden frames |
| [Warm runtime](warm-runtime.md) | Pre-warming on project start, warm-document reuse, isolation between previews and resource bounds |
| [Readiness](readiness.md) | What readiness waits for, phase diagnostics, rendering hidden frames |
| [Plugins](plugins.md) | Workbench's format handling as Rspack loaders and plugins, and the project plugin contract in `workbench.config.ts` |

## Decisions

- Rspack is the development compiler, with hot updates for every native
  format, shared chunks, pushed build results and a warm document. Portable
  output uses an Rspack production build with the same project plugin form.
- The compiler declares and builds every entry before serving its first
  completed generation. A replacement start must preserve chunk stability,
  freshness, errors and hot updates while meeting the first-page target.
- The native page index lists independently of compilation. It combines
  directory/file-stamp reuse, conservative literal parsing and compiler
  evaluation of other definitions.
- Warm first-page rendering and complete listing must meet the agreed
  [performance targets](performance.md#measured-interactions) at any page
  count. Proposed optimizations are evaluated against correctness and the
  existing interaction regression limits in [labs](../labs.md).
- Rust is the direction for suitable host capabilities in the extension and
  Studio. Electron owns screenshot rendering. An additional Rust component
  needs a defined integration contract; a lab prototype does not establish it.
- The Workbench server becomes one Rust runtime, with the page index and the
  compiler, and Node only as an evaluator of project code; Astro and React
  Native Web are required ([adapters](../adapters/README.md#decisions)).

## Status

| Capability | Implementation and evidence boundary |
| --- | --- |
| Native page listing | Implemented in `server/` ([catalog.rs](../../server/src/catalog.rs) reads its answer); [acceptance](page-index.md#acceptance-and-evidence) maps behavior to tests and identifies gaps. [Server timings](performance.md#page-listing) do not establish canvas-paint acceptance |
| Rust engineering gates | Implemented; [code quality](../code-quality.md) owns tools, policy, test conventions and CI enforcement |
| Rspack compilation and pre-warming | Implemented in `workbench-server` with Rspack's Rust crates, one compiler per space and profile, pre-warming every listed preview and docs lens once the page tree configures them; project JavaScript runs in the Node evaluator over stdin and stdout ([server](../server.md#previews), [compiler](compiler.md)). The [instant first page lab](../../labs/instant-first-page/README.md#stage-2-report)'s targets are not yet measured against it |

The implemented page index follows this [design direction](#decisions).
It lists pages independently of the compilers and the Node evaluator; it
does not change the compiler's entry preparation or pre-warming.

The Rspack preview pipeline is implemented in
[server/src/compiler.rs](../../server/src/compiler.rs) and its modules, the
Node evaluator, and the preview runtime, canvas and docs page. Each topic's
status names what remains. The implementation passes the package's tests.
The [Rspack pipeline lab](../../labs/rspack-pipeline/README.md)'s stage 1
checks on the formats fixture have not been run against this
implementation. The confirmation run the
[labs methodology](../labs.md#reports-and-decisions) asks for, the lab's
sequences through the shipping server and the VS Code webview, has not been
made yet.

Design evidence and raw comparisons live in the
[Rspack pipeline lab](../../labs/rspack-pipeline/README.md#stage-2-report).
Current product measurements and acceptance limits belong to
[performance](performance.md#status).

## Open questions

Each topic owns its unresolved decisions. The ones that change the plan:

- Native listing residency, watching, trust and Studio integration
  ([page index](page-index.md#remaining-work)).
- How to show the requested first page within the budget without waiting
  for catalog-size-dependent entry preparation and compilation
  ([compiler](compiler.md#open-questions)).

- How readiness waits for fonts a page starts loading
  ([readiness](readiness.md#open-questions)).
- How the entry set changes when a preview or docs lens is added or removed
  while pages are open ([compiler](compiler.md#open-questions)).
