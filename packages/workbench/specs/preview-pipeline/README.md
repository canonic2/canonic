# Preview pipeline

The preview pipeline is how Workbench turns a project's native pages
(TypeScript previews and docs) into something the canvas shows. It compiles
them, serves the output, loads it into a frame, mounts it, waits for it to be
ready, and applies edits. [TypeScript previews](../pages/previews.md) and
[docs](../pages/docs-pages.md) own what a preview or docs page is and does.
This folder owns how they are built, delivered and kept current.

## The pipeline

1. **Discover and list.** The native page index walks the project, parses
   literal definitions, and evaluates computed definitions and docs in a
   separate Node process. A saved index avoids re-reading unchanged inputs
   ([page index](page-index.md)).
2. **Evaluate.** The Node evaluator runs definitions and
   `workbench.config.ts`, which are project code. The index batches its evaluated
   definitions; compilation uses a long-lived evaluator for project settings,
   definitions and adapter jobs ([compiler](compiler.md#what-runs-in-the-node-evaluator),
   [server processes](../system/server.md#processes-and-connections)).
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
   loaded the runtime and shared code mounts the preview
   ([warm runtime](warm-runtime.md)). Docs use a dedicated
   pool that preserves recent page documents and reuses idle documents for new
   pages ([Docs host](docs-host.md)).
6. **Ready.** After mount and hooks, readiness waits for fonts and visible images
   ([readiness](readiness.md)).
7. **Update.** The server pushes build results to the canvas. Open frames apply
   hot updates in place, or reload when an edit cannot apply
   ([hot updates](hot-updates.md)).

Project plugins and Workbench's own format handling are Rspack loaders and
plugins ([plugins](plugins.md)). The [performance](../system/performance.md) spec owns
the targets the whole pipeline is measured against.

## Topics

Each topic states its status in a paragraph at its top.

| Spec | Covers |
| --- | --- |
| [Page index](page-index.md) | Complete ordered listing, definition parity, saved-state invalidation, process and storage bounds, failure recovery and acceptance evidence |
| [Compiler](compiler.md) | Rspack compilers per project and profile, configuration from the page tree, the fixed entry set, in-memory output, persistent caching, config changes, build errors, packaging and the jobs it sends the Node evaluator |
| [First page](first-page.md) | Design constraints for a compiler start that serves the first page without building every entry |
| [Serving](serving.md) | Output URLs and caching, the preview module, stylesheet names, which requests wait for a build |
| [Hot updates](hot-updates.md) | The push channel, edit behavior per format, the reload rule, state kept across edits, build errors and recovery, hidden frames |
| [Warm runtime](warm-runtime.md) | Pre-warming, preview reuse, isolation, the retained content-session budget and resource bounds |
| [Docs host](docs-host.md) | Docs serving and highlighting, the warm Docs pool, the Docs return check, Docs isolation, the reusable page lifecycle and Docs implementation constraints |
| [Readiness](readiness.md) | What readiness waits for and its constants, phase diagnostics, rendering hidden frames |
| [Plugins](plugins.md) | Workbench's format handling as Rspack loaders and plugins, and the project plugin contract in `workbench.config.ts` |

Shared [performance budgets and measurement rules](../system/performance.md)
live under system contracts. [Research](../../research/README.md) owns dated
observations and lab/host evidence; neither is a second pipeline implementation contract.

## Decisions

Each decision lives with the topic that owns it:

- Rspack as the development and portable compiler: [compiler](compiler.md#decisions).
- The eager start and its first-page gap: [performance gaps](../system/performance.md#implementation-gaps);
  a proposed replacement start: [first page](first-page.md).
- Listing independently of compilation: [page index](page-index.md#purpose-and-ownership).
- First-page and complete-list targets at any page count, and how
  optimizations are evaluated: [performance](../system/performance.md#measured-interactions)
  and [labs](../system/labs.md).
- Rust for host capabilities and Electron for screenshot rendering:
  [server decisions](../system/server.md#decisions-for-the-owner).
- One Rust server runtime with Node only as an evaluator: [adapters](../adapters/README.md#decisions).

## Open questions

Each topic owns its unresolved decisions:

- [Page index](page-index.md#gaps)
- [Compiler](compiler.md#open-questions)
- [First page](first-page.md#open-questions)
- [Readiness](readiness.md#open-questions)
- [Warm runtime](warm-runtime.md#open-questions)
- [Plugins](plugins.md#open-questions)
