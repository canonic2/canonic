# Hot updates

Part of the [preview pipeline](README.md). This spec covers how an edit
reaches the pages on the canvas: the push channel, what each kind of edit
does to an open page, and how errors recover. The edit requirements are in
[performance](../system/performance.md#edits-apply-in-place); this spec defines how the
pipeline meets them.

## Requirements

### Push

- The server pushes build results to the canvas over one event stream per
  open canvas. Preview frames receive them through the canvas, sharing that
  stream, and open no connection of their own.
- No frame starts periodic revision polling. Edit latency depends on the build
  and update, not on an interval. One-shot checks on retained return protect
  against missed updates ([warm runtime](warm-runtime.md)).
- The stream carries:

  | Event | Meaning |
  | --- | --- |
  | `built` | A compiler finished a build: its profile, generation, hash, and build errors per entry |
  | `docs-changed` | A docs page's Markdown changed |
  | `reload-all` | The compilers were replaced; documents from older generations reload |

- The stream sends a heartbeat every 15 seconds, so a broken connection is
  noticed and reopened. A reopened stream starts with the current state, so a
  frame that missed a build catches up.

### Frames that apply updates

- Every open frame whose document runs a profile's runtime applies that
  profile's hot updates as they arrive: the visible frame, the spare and
  hidden retained frames. Push keeps retained frames current during normal
  operation; return-time revision checks cover updates missed while retained.
- A frame applies updates until its document runs the build hash the `built`
  event named. An update that Rspack aborts or fails, or a document left on an
  older generation, reloads the preview into the warm host.
- Hidden frames render behind the visible one ([readiness](readiness.md#hidden-frames)).
  Their CPU and memory costs count against the retained-runtime budget.

### Edit behavior

What each edit does to an open page. "State kept" means component state,
such as a counter or typed text, survives. The selected scenario, size and input
overrides survive every row, including reloads.

| Edit | Behavior |
| --- | --- |
| Global CSS, a CSS module, a Vue style block, an HTML preview's linked stylesheet | Stylesheet replaced in place; state kept |
| A React component | React Fast Refresh; hooks' state kept while the hooks' order is unchanged |
| A module exporting values other than components | Hot update through the modules that accept it; Fast Refresh re-renders the affected components, or re-mounts their tree when it cannot apply |
| A Vue component's template | Vue's hot reload re-renders; state kept |
| A Vue component's script | Vue's hot reload re-creates the component; its state resets, as Vue defines |
| An HTML preview's markup | The preview re-renders in place from the new module, without a document reload |
| An Astro component | The Node evaluator renders the preview again on the server, and its HTML replaces the old in place without a document reload ([Astro adapter](../adapters/astro.md#edits)) |
| A docs page's Markdown | The page's Markdown is replaced in place; live examples and scroll position kept ([docs](#docs-pages)) |
| A docs example's source | The example updates in place through its adapter, as the matching preview edit would |
| A module shared by many pages | Hot update in every open frame that loaded it |
| A preview definition, the project or preview environment | Preview reload ([the reload rule](#the-reload-rule)) |
| `workbench.config.ts` | Compilers replaced and every document reloaded ([configuration changes](#configuration-changes)) |

Astro islands are outside this table, because Workbench rejects them
([previews](../pages/previews.md)).

### How an entry applies an update

- Each entry accepts hot updates to its preview's sources. On an update it
  asks the runtime to render again with the new sources and the last
  overrides, so the mounted preview keeps its selected scenario, size and inputs.
- Framework components below the sources update through their own hot
  handling (React Refresh, Vue's hot reload) before the entry sees anything.
- An update that reaches no accepting module bubbles to the entry, which
  re-renders.

### The reload rule

These edits cannot apply in place and reload the preview into its warm host,
keeping the selected scenario, size and inputs:

- the preview definition, because it can change scenarios, inputs and controls;
- the project or preview environment, because providers wrap the whole tree;
- a module with side effects that accepts no hot update, when the update
  reaches no accepting module.

A preview reload is a remount in the same warm host, not a document reload.
Component state is lost.

### Configuration changes

A change to `workbench.config.ts` replaces the compilers
([compiler](compiler.md#configuration-changes)) and sends `reload-all`. A
warm host on an older generation navigates to a fresh warm host document and
mounts its preview there. Any other document reloads.

### Docs pages

- On `docs-changed`, a docs page fetches its rendered page again and
  replaces the Markdown content around its live examples. Example figures
  that are still on the page keep their mounted examples. Scroll position is
  kept.
- Example edits arrive as hot updates to the docs entry, like preview edits.
  After successfully applying a build, the hot client records that entry's
  examples revision on the live page, preserving its Markdown revision. Failed
  or superseded updates cannot acknowledge an unapplied revision.
- Examples revisions use the entry's reachable module content hashes, errors
  and compiler generation. Adding unrelated entries or repartitioning shared
  chunks does not change that identity.
- A retained Docs return is shown immediately, then checks its revision once
  in the background to catch a missed push. An unchanged page stays mounted;
  a changed page follows these same update rules with scroll preserved.
  This is a return-time check, not periodic polling. Superseded or disposed
  lifecycles ignore late check/update results. The
  [warm Docs contract](warm-runtime.md#resume-and-freshness) owns failure and
  replacement behavior. Return revision checks are implemented alongside the
  push path; [integration evidence](../../research/docs-navigation.md#warm-docs-integration-2026-10-23)
  records their verification scope.

### Errors and recovery

- A build error shows in the frame of each affected entry as soon as the
  build reports it, within 1 second. Frames of other entries keep applying
  updates.
- A preview opened while its last build failed shows the error at once
  ([serving](serving.md#the-preview-module)).
- The next successful build recovers the frame without a manual reload, by
  hot update or, when the update cannot apply, by a preview reload.
- Thrown errors during an update follow the runtime error contract: readiness
  clears and the frame shows the error ([previews](../pages/previews.md#readiness-and-errors)).
- When saves follow each other quickly, the page ends with the last save's
  output.

## Status

Implemented: the push channel, hot updates for every native format, the reload
rule, errors per entry and docs pages updated in place. Gaps:

- Docs entries register their example modules but do not add the preview
  entry's source-accept callback ([entries.rs](../../server/src/compiler/entries.rs)).
  Framework/module hot handlers can update in place; an unaccepted example
  change uses the warm Docs host's fallback and can remount examples. Do not
  claim component state survives every Docs example edit.
- A preview or docs lens added while pages are open joins the running
  compilers, which can move shared chunks; open pages can then miss hot
  updates until they reload ([compiler](compiler.md#open-questions)).
- A reopened event stream does not start with the current state; a frame that
  missed a build catches up at the next one.

## Verification

The compiler's Rust unit tests (in [compiler.rs](../../server/src/compiler.rs)'s
modules, such as the hot client in [client.rs](../../server/src/compiler/client.rs)),
[preview integration tests](../../server/node/preview.test.js) and
[docs integration tests](../../server/node/preview-docs.test.js) exercise the implemented
build, serving and docs paths. The [pipeline research](../../research/rspack-pipeline.md#stage-2-2026-10-13)
holds format-by-format edit and isolation evidence. Event-stream reconnects
must still be tested against missed builds, and entry-set replacement must
preserve hot updates in already-open documents.
