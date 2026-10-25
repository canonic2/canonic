# Storybook contract

Storybook is an opt-in implementation. A project maps an authored page in
`workbench.yaml` to one exact Storybook title; Storybook does not add pages
of its own. How the Workbench server runs Storybook is in the
[Storybook adapter](../adapters/storybook.md).
Selection, addresses, and lens persistence are in [core.md](../core.md); URL
implementations, code pointers, and the page compatibility script are in
[implementations.md](implementations.md).

**Status:** title mapping, story lookup, channel switching with its navigation
fallback, retained Storybook sessions, capture and export are implemented. A
stopped or failed Storybook is not reported in `problems` up front; see
[current gaps](#current-gaps).

## Configuration

The keys, their validation, and examples are in the
[workbench.yaml reference](../../docs/configuration.md#implementations) and the
[Storybook guide](../../docs/storybook.md); this spec does not repeat them.

- A project may declare several Storybooks. Workbench reads each one's live
  `/index.json`, which Storybook 7 and later serve. Story source links and
  export sources resolve against the implementation's `root`.
- Without `url`, the Workbench server runs the project's Storybook itself
  from `root` (the space root unless set), on a local socket and never a
  TCP port, in trusted workspaces only
  ([adapter](../adapters/storybook.md#a-storybook-the-server-runs)). `start`
  without `url`, and `base` on a Storybook, are problems.
- `url: auto` finds a running Storybook from the project's package scripts and
  the default ports ([guide](../../docs/storybook.md#detect-the-port-with-url-auto)).
  The detected address is remembered while the server runs. When nothing
  answers, `problems` says so and story lookups for that implementation are
  refused. Detection never launches a process.
- `start`, for a Storybook at `url`, launches it in VS Code as described in
  [implementation startup](../integrations/vscode-extension.md#implementation-startup). A
  timeout is logged; the canvas still opens, and a mapped page's Storybook
  lens shows the failure when it is opened.
- `catalog` on a Storybook implementation is a problem
  (`implementations › <key>: catalog is only available for iOS Simulator
  implementations.`).

## Story identity

- Only index entries with `type: story`, a string ID, and a title become
  selectable stories; docs entries are skipped. Entries are grouped by their
  exact `title`.
- A story's workbench scenario is the part of its ID after `--`; the address
  stores that scenario and the story keeps its full Storybook ID. The preview URL
  is `<url>/iframe.html?id=<story-id>&viewMode=story`. **Open on its own**
  opens the same story inside Storybook at `<url>/?path=/story/<story-id>`.
- An authored page can map one implementation to a title, for example
  `storybook: UI/Components/Button`. Story lookup matches that title exactly;
  a similar final segment under another prefix is not silently substituted.
  When only the prefix differs, the lookup error suggests the matching titles.
- The Storybook lens shows the title's first story, or the one the address
  names. The top bar's scenario switcher lists the title's stories in place of
  the design's scenarios.
- The server fetches stories for the selected title through
  `/_workbench/stories`. The canvas keeps each title's list for the session in
  one catalog cache keyed by implementation and exact title. The outer UI and
  content renderers share pending and completed lookups over source-, origin-
  and channel-checked messages. Failed lookups can retry; refreshing
  configuration clears the shared cache. Closing a renderer cancels its pending
  catalog responses and deadlines. Source paths come from Storybook's `componentPath` and `importPath`,
  resolved against `root` and kept only when they exist; a page's own
  `code` pointers are added after them. A failed lookup shows its error on
  the canvas, and a stopped Storybook does not remove other usable pages.
- The Actions switch is disabled and shown as on through a Storybook lens.

## Switching the visible story

The [multiple-artboard integration contract](../canvas/multiple-artboards.md) requires two
stories from the same Storybook to remain independently rendered. Story reuse,
channel messages, pending acknowledgements, and session retention must be scoped
to an artboard instance and render generation, not just implementation/base URL.
A message from one instance cannot switch another or steal canvas selection.
This work follows the iframe runtime ownership in the
[code plan](../canvas/artboard-system.md); the renderer behavior below applies independently within each artboard.

- The first story loads in the preview iframe. Later picks on the same
  Storybook origin ask the loaded preview to switch through Storybook's
  `setCurrentStory` channel. This keeps the Storybook runtime warm.
- Transitions between stories, pages and lenses follow [navigation](../canvas/navigation.md),
  including its grace period while the story catalog is unresolved and its single
  **Loading preview…** status for catalog lookup and rendering. Story lookup emits
  no progress toast; a lookup failure is the destination's failure, and retrying
  the failed title starts a fresh status.
- Ready status requires the matching story's render event and visible iframe;
  iframe document loading alone does not make a story ready.
- Titles, scenario controls and renderer labels derive from the current page and
  resolved story metadata. An unresolved story leaves no previous scenario name
  behind; a resolved story updates those names before preview switching starts,
  and a label update cannot acknowledge an unrendered story.
- Returning from explicit artboards restores the
  [parked ordinary renderer's](../canvas/multiple-artboards.md#ui-and-integration-scope)
  Storybook iframe without loading another preview document.
- Leaving a Storybook lens retains its iframe under the
  [retained-session budget](../preview-pipeline/warm-runtime.md#retained-sessions).
  Returning to its current story preserves interactions without rendering again.
  Picking another story in that session uses the channel; the session retains
  the most recently rendered story.
- Workbench accepts `currentStoryWasSet` and `storyRendered` only from the
  active or loading preview iframe, at the resolved Storybook proxy origin, and for
  the requested story ID. It reports the selection to the VS Code sidebar and
  signals a new frame to capture only after `storyRendered`. A pending
  selection keeps the editor bridge ready for another pick without claiming
  the old canvas has changed.
- If Storybook does not acknowledge a switch within 1.5 seconds, or accepts
  it but does not report rendering within 15 seconds, Workbench navigates to
  that story's `iframe.html` URL. A newer pick cancels the older fallback.
  An initial load, a pending iframe navigation, or a different Storybook
  origin also uses iframe navigation.
- Editor shortcuts reach the workbench through Storybook's `previewKeydown`
  channel, accepted only from the active preview iframe at its resolved proxy
  origin. Storybook itself does not forward keys while a text field in the
  story has focus (reported in the user guide; not controlled by Workbench).

## Screenshots and export

Storybook renders as [content only](../system/rendering-flow.md): a dedicated renderer
loads its proxied `iframe.html`, never Workbench's application `index.html`.
Changing or returning to a story preserves the retained Storybook content
document/channel where allowed; retaining a nested Workbench shell is not the
mechanism.

- A screenshot of a story captures the live story, including opened modals,
  typed text, and scroll, with no Storybook changes: the
  [implementation proxy](../system/implementation-proxy.md) adds the preview bridge to
  `iframe.html`. Stories must not be framed from Storybook's own address or
  served from the workbench's origin instead; see that spec. Capture is in
  [capture.md](../review/capture.md).
- Design-system export captures a page mapped to Storybook as its design. An
  export whose lens is the Storybook implementation captures each mapped
  page's stories instead, at each of the page's resolved sizes; Resizable is
  skipped under the [size export rule](../canvas/sizes.md#design-system-export).
  [Export capture](../review/export.md#reference-capture-plan) schedules background
  workers and does not drive the visible workbench preview.

## Decisions

- **Exact titles.** A mapping names the title Storybook shows. Matching a
  final segment under another prefix could pick the wrong component, so the
  lookup suggests it instead of substituting it.
- **Channel switching with navigation fallback.** Reusing the loaded preview
  makes switching fast, but the channel does not confirm delivery, so
  Workbench navigates when acknowledgement or rendering doesn't arrive.

## Current gaps

- **Storybooks are not checked up front.** Only failed `url: auto` detection,
  and a Storybook the server runs in an untrusted workspace or without a
  usable root or Node runtime, add to `problems`. When a Storybook at an
  explicit `url` is stopped, or one the server runs fails to start, the
  failure appears on the canvas when a mapped page is opened, not in
  `problems`, short of the shared problems list in
  [core.md](../core.md#problem-reporting).

## Open questions

- The detected `url: auto` address and the canvas's story lists are kept until
  the server or canvas reloads. Should a Storybook that moves ports, or a
  title that gains stories, be picked up without a reload?

## Verification points

- [server.test.js](../../server/node/server.test.js) checks port discovery
  from package scripts, automatic detection, exact-title lookup with code
  paths, the not-running and not-a-Storybook errors, and export plans.
- [manifest.test.js](../../contracts/manifest.test.js) and
  [config.test.js](../../server/node/config.test.js) check automatic implementations, the `catalog` problem, start settings,
  and a story's scenario from its ID.
- [lenses.test.js](../../ui/legacy/lenses.test.js) checks story URLs, address
  picks, and channel messages.
  [preview.test.js](../../ui/legacy/preview.test.js) checks channel reuse,
  navigation fallback, and fallback reporting.
  [keys.test.js](../../ui/legacy/keys.test.js) checks that editor shortcuts are
  accepted only from the active Storybook iframe and its origin.
- [startup.test.js](../../extension/startup.test.js) checks start commands, trust, and
  readiness probes.
- [smoke-preview-sessions.cjs](../../scripts/smoke-preview-sessions.cjs) drives the
  production canvas with a channel-compatible Storybook fixture. It checks one
  preview load across mapped pages, story/lens switches and explicit artboards,
  one index lookup per title, retained edits, delayed rendering,
  painted page/story/artboard labels, continuous loading through delayed lookup
  and rendering, failure/retry and
  empty selection, editor acknowledgements, superseded picks, hidden cold
  startup and Reload.
- No check verifies that the Storybook renderer never starts Workbench's
  application; add one before claiming the content-only boundary.
