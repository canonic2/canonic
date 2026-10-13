# Hot updates

Part of the [preview pipeline](README.md). This spec covers how an edit
reaches the pages on the canvas: the push channel, what each kind of edit
does to an open page, and how errors recover. The edit requirements are in
[performance](performance.md#edits-apply-in-place); this spec defines how the
pipeline meets them.

## Requirements

### Push

- The server pushes build results to the canvas over one event stream per
  open canvas. Preview frames receive them through the canvas, sharing that
  stream, and open no connection of their own.
- No frame polls for a revision. Edit latency depends only on the build and
  the update, not on an interval.
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
  hidden retained frames. A retained frame shown again is already current,
  so it needs no check on return.
- A frame applies updates until its document runs the build hash the `built`
  event named. An update that Rspack aborts or fails, or a document left on an
  older generation, reloads the preview into the warm host.
- Hidden frames render behind the visible one ([readiness](readiness.md#hidden-frames)).
  Applying updates there produced no console errors in stage 1. Their CPU and
  memory cost is measured in stage 2.

### Edit behavior

What each edit does to an open page. "State kept" means component state,
such as a counter or typed text, survives. The selected state, size and input
overrides survive every row, including reloads.

| Edit | Behavior |
| --- | --- |
| Global CSS, a CSS module, a Vue style block, an HTML preview's linked stylesheet | Stylesheet replaced in place; state kept |
| A React component | React Fast Refresh; hooks' state kept while the hooks' order is unchanged |
| A module exporting values other than components | Hot update through the modules that accept it; Fast Refresh re-renders the affected components, or re-mounts their tree when it cannot apply |
| A Vue component's template | Vue's hot reload re-renders; state kept |
| A Vue component's script | Vue's hot reload re-creates the component; its state resets, as Vue defines |
| An HTML preview's markup | The preview re-renders in place from the new module, without a document reload |
| An Astro component | The worker renders the preview again on the server, and its HTML replaces the old in place without a document reload ([plugins](plugins.md#astro)) |
| A docs page's Markdown | The page's Markdown is replaced in place; live examples and scroll position kept ([docs](#docs-pages)) |
| A docs example's source | The example updates in place through its adapter, as the matching preview edit would |
| A module shared by many pages | Hot update in every open frame that loaded it |
| A preview definition, the project or preview environment | Preview reload ([the reload rule](#the-reload-rule)) |
| `workbench.config.ts` | Compilers replaced and every document reloaded ([configuration changes](#configuration-changes)) |

Astro islands are outside this table, because Workbench rejects them
([previews](../previews.md)).

### How an entry applies an update

- Each entry accepts hot updates to its preview's sources. On an update it
  asks the runtime to render again with the new sources and the last
  overrides, so the mounted preview keeps its selected state, size and inputs.
- Framework components below the sources update through their own hot
  handling (React Refresh, Vue's hot reload) before the entry sees anything.
- An update that reaches no accepting module bubbles to the entry, which
  re-renders.

### The reload rule

These edits cannot apply in place and reload the preview into its warm host,
keeping state, size and inputs:

- the preview definition, because it can change states, inputs and controls;
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

### Errors and recovery

- A build error shows in the frame of each affected entry as soon as the
  build reports it, within 1 second. Frames of other entries keep applying
  updates.
- A preview opened while its last build failed shows the error at once
  ([serving](serving.md#the-preview-module)).
- The next successful build recovers the frame without a manual reload, by
  hot update or, when the update cannot apply, by a preview reload.
- Thrown errors during an update behave as runtime errors do today: readiness
  clears and the frame shows the error ([previews](../previews.md#readiness-and-errors)).
- When saves follow each other quickly, the page ends with the last save's
  output.

## Status

Implemented: the push channel, hot updates for every native format, the reload
rule, errors per entry and docs pages updated in place. Gaps:

- A preview or docs lens added while pages are open joins the running
  compilers, which can move shared chunks; open pages can then miss hot
  updates until they reload ([compiler](compiler.md#open-questions)).
- A reopened event stream does not start with the current state; a frame that
  missed a build catches up at the next one.

## Discoveries

- **2026-10-12, stage 1** of the [Rspack pipeline lab](../../labs/rspack-pipeline/README.md#stage-1-report),
  formats fixture, one sample per edit, Intel macOS, headless Chrome:
  - Every row of the edit table behaved as written.
  - Stylesheet edits showed in 52–80 ms, React and Vue edits in 52–189 ms,
    HTML and Astro re-renders in 52–80 ms, and docs edits in 51–80 ms.
  - Definition and environment edits reloaded the preview in 103–133 ms.
    Config edits replaced the compilers in 375–384 ms.
  - A syntax error showed in the frame and recovered in 266–317 ms after the
    fix. Two saves 150 ms apart ended on the second.
  - The shipping control took 0.64–1 s for every edit, lost component state,
    and did not recover from the syntax error within 30 s.
  - With no edits, no revision requests were made in 5 s with every page
    open; the control polled.
- **2026-10-12, stage 1, edit to a module shared by every page.** A hidden
  retained frame showed the edit when selected again, without a reload.
- **2026-10-11, preview loading lab.** Rspack with HMR kept React state on
  every edit, with edits visible in 0.05–0.16 s on the 40-page fixture
  ([report](../../labs/preview-loading/README.md#hmr)).
