# Preview pipeline

The preview pipeline is how Workbench turns a project's native pages
(TypeScript previews and docs) into something the canvas shows. It compiles
them, serves the output, loads it into a frame, mounts it, waits for it to be
ready, and applies edits. These specs define the target pipeline and how
Workbench gets there. [TypeScript previews](../pages/previews.md) and
[docs](../pages/docs-pages.md) still own what a preview or docs page is and does.
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

| Spec | Covers |
| --- | --- |
| [Page index](page-index.md) | Complete ordered listing, definition parity, saved-state invalidation, process and storage bounds, failure recovery and acceptance evidence |
| [Compiler](compiler.md) | Rspack compilers per project and profile, configuration from the page tree, the fixed entry set, in-memory output, persistent caching, config changes, build errors, packaging and the jobs it sends the Node evaluator |
| [First page](first-page.md) | Proposed, not agreed: design constraints for a compiler start that serves the first page without building every entry |
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
- The eager start and its first-page gap: [performance status](../system/performance.md#status);
  a proposed replacement start: [first page](first-page.md).
- Listing independently of compilation: [page index](page-index.md#purpose-and-ownership).
- First-page and complete-list targets at any page count, and how
  optimizations are evaluated: [performance](../system/performance.md#measured-interactions)
  and [labs](../system/labs.md).
- Rust for host capabilities and Electron for screenshot rendering:
  [server decisions](../system/server.md#decisions-for-the-owner).
- One Rust server runtime with Node only as an evaluator: [adapters](../adapters/README.md#decisions).

## Status

| Capability | Implementation and evidence boundary |
| --- | --- |
| Native page listing | Implemented in `server/` ([catalog.rs](../../server/src/catalog.rs) reads its answer); [acceptance](page-index.md#acceptance-and-evidence) maps behavior to tests and identifies gaps. [Server timings](../../research/page-listing.md#page-listing) do not establish canvas-paint acceptance |
| Rust engineering gates | Implemented; [code quality](../system/code-quality.md) owns tools, policy, test conventions and CI enforcement |
| Warm Docs pool | Implemented: immediate retained resume, background revision checks, parallel metadata and bounded same-lens idle reuse. [Integration evidence](../../research/docs-navigation.md#warm-docs-integration-2026-10-23) separates focused tests, shipping measurements and installed Acme confirmation |
| Rspack compilation and pre-warming | Implemented in `workbench-server` with Rspack's Rust crates, one compiler per space and profile, pre-warming every listed preview and docs lens once the page tree configures them (at startup when the host passes `--eager-previews`, [warm runtime](warm-runtime.md#status)); project JavaScript runs in the Node evaluator over stdin and stdout ([server](../system/server.md#previews), [compiler](compiler.md)). Scoped Docs and startup measurements exist; the complete-list/first-page target campaign remains open |

The implemented page index follows this [design direction](#decisions).
It lists pages independently of the compilers and the Node evaluator; it
does not change the compiler's entry preparation or pre-warming.

The Rspack preview pipeline is implemented in
[server/src/compiler.rs](../../server/src/compiler.rs) and its modules, the
Node evaluator, and the preview runtime, canvas and docs page. Each topic's
status names what remains. Automated evidence belongs to each topic's verification section.
The [Rspack pipeline research](../../research/rspack-pipeline.md)'s stage 1
checks on the formats fixture have not been run against this
implementation. The confirmation run the
[labs methodology](../system/labs.md#reports-and-decisions) asks for, the lab's
full format-by-format sequences through the shipping server and the VS Code
webview, remains open. The narrower warm Docs confirmation and installed Acme
checks are complete, with limits recorded in [Docs navigation evidence](../../research/docs-navigation.md#warm-docs-integration-2026-10-23).

Design evidence and comparisons live in the
[Rspack pipeline research](../../research/rspack-pipeline.md#stage-2-2026-10-13).
Current product measurements and acceptance limits belong to
[performance](../system/performance.md#status).

## Open questions

Each topic owns its unresolved decisions:

- [Page index](page-index.md#remaining-work)
- [Compiler](compiler.md#open-questions)
- [First page](first-page.md#open-questions)
- [Readiness](readiness.md#open-questions)
- [Warm runtime](warm-runtime.md#open-questions)
- [Plugins](plugins.md#open-questions)
