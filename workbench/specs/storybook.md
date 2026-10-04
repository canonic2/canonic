# Storybook contract

Storybook is an opt-in implementation. A project can map an authored page to
one exact Storybook title, import a whole Storybook catalog, or do both.
Selection, addresses, and lens persistence are in [core.md](core.md); URL
implementations, code pointers, and the page compatibility script are in
[implementations.md](implementations.md).

## Configuration

The keys, their validation, and examples are in the
[workbench.yaml reference](../docs/configuration.md#implementations) and the
[Storybook guide](../docs/storybook.md); this spec does not repeat them.

- A project may declare several Storybooks. Workbench reads each one's live
  `/index.json`, which Storybook 7 and later serve. Story source links and
  export sources resolve against the implementation's `root`.
- `url: auto` finds a running Storybook from the project's package scripts and
  the default ports ([guide](../docs/storybook.md#detect-the-port-with-url-auto)).
  The detected address is remembered while the server runs. When nothing
  answers, `problems` says so and story lookups for that implementation are
  refused. Detection never launches a process.
- `start` launches Storybook in VS Code as described in
  [implementation startup](vscode-extension.md#implementation-startup). A
  timeout is logged; the canvas still opens and the catalog reports its own
  problem.
- `catalog: true` imports the Storybook index. Without it, only explicitly
  mapped authored pages use Storybook.

## Catalog and story identity

- Catalog page icons use the longest matching `catalog.icons` title prefix,
  then `catalog.icon`, then `book-open`. Prefixes match complete `/` segments.
  Collection lookup uses the collection name and retains whether its icon came
  from a mapping, a configured fallback, or the built-in default. Shared
  collections use the
  [preview placement precedence](previews.md#sidebar-placement); conflicting
  catalogs at equal priority choose the lexically first icon name.

- Only index entries with `type: story`, a string ID, and a title become
  selectable stories; docs entries are skipped. Entries are grouped by their
  exact `title`.
- For a title such as `UI/Forms/Inputs/Text field`, `UI` becomes a collection,
  `Forms / Inputs` becomes one group (groups don't nest), and `Text field`
  becomes a page. A one-segment title goes in a collection named after the
  implementation's label. Each story is one of the page's states, labeled
  with the story's name; a one-story title is a page that doesn't expand.
- An imported collection joins an authored collection with the same name.
  Imported pages have a synthetic
  `__storybook/<implementation>/<component>.html` address, open straight on the Storybook lens, and have no Design lens.
  Groups with exactly the same name within that collection join the existing
  group, including groups supplied by TypeScript previews. Pages retain
  their own addresses, states, and lenses.
- A story's workbench state is the part of its ID after `--`; the address
  stores that state and the story keeps its full Storybook ID. The preview URL
  is `<url>/iframe.html?id=<story-id>&viewMode=story`. **Open on its own**
  opens the same story inside Storybook at `<url>/?path=/story/<story-id>`.
- An authored page can map one implementation to a title, for example
  `storybook: UI/Components/Button`. Story lookup matches that title exactly;
  a similar final segment under another prefix is not silently substituted.
  When only the prefix differs, the lookup error suggests the matching titles.
- The Storybook lens shows the title's first story, or the one the address
  names. On mapped and imported pages alike, the top bar's state switcher
  lists the title's stories in place of the design's states.
- The server fetches stories for the selected title through
  `/_workbench/stories`. The canvas keeps each title's list for the session.
  Source paths come from Storybook's `componentPath` and `importPath`,
  resolved against `root` and kept only when they exist; a page's own
  `code` pointers are added after them. A failed lookup shows its error on
  the canvas, and a stopped Storybook does not remove other usable pages.
- The Actions switch is disabled and shown as on through a Storybook lens.

## Switching the visible story

The future [multiple-artboard contract](multiple-artboards.md) requires two
stories from the same Storybook to remain independently rendered. Story reuse,
channel messages, pending acknowledgements, and session retention must be scoped
to an artboard instance and render generation, not just implementation/base URL.
A message from one instance cannot switch another or steal canvas selection.
This work follows the iframe runtime ownership in the
[code plan](multiple-artboards-code-plan.md); the renderer behavior below applies independently within each artboard.

- The first story loads in the preview iframe. Later picks on the same
  Storybook origin ask the loaded preview to switch through Storybook's
  `setCurrentStory` channel. This keeps the Storybook runtime warm.
- Leaving a Storybook lens retains its iframe under the canvas's 15-minute,
  three-inactive-session policy. Returning to its current story preserves
  interactions without rendering again. Picking another story in that session
  uses the channel; the session retains the most recently rendered story.
- Workbench accepts `currentStoryWasSet` and `storyRendered` only from the
  active or loading preview iframe, at the configured Storybook origin, and for
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
  channel, accepted only from the active preview iframe at the Storybook's
  origin. Storybook itself does not forward keys while a text field in the
  story has focus (reported in the user guide; not controlled by Workbench).

## Screenshots and export

- A screenshot of a story captures the live story, including opened modals,
  typed text, and scroll, with no Storybook changes: the
  [implementation proxy](implementation-proxy.md) adds the preview bridge to
  `iframe.html`. Stories must not be framed from Storybook's own address or
  served from the workbench's origin instead; see that spec. Capture is in
  [capture.md](capture.md).
- Design-system export plans a reference for each imported story at each of
  the page's viewports; imported pages declare none, so they use fit,
  desktop, and mobile. A state without a story ID becomes a warning. An
  authored page mapped to Storybook exports its design, not the story.
  [Export capture](export.md) schedules background workers and does not drive
  the visible workbench preview.

## Decisions

- **Exact titles.** A mapping names the title Storybook shows. Matching a
  final segment under another prefix could pick the wrong component, so the
  lookup suggests it instead of substituting it.
- **Channel switching with navigation fallback.** Reusing the loaded preview
  makes switching fast, but the channel does not confirm delivery, so
  Workbench navigates when acknowledgement or rendering doesn't arrive.

## Current gaps

Found on 2026-10-03 by reading the code.

- **Mapped-only Storybooks are not checked up front.** Only catalog imports
  and `url: auto` detection add to `problems`. When a Storybook used only by
  mapped pages is stopped, the failure appears on the canvas when such a
  page is opened, not in `problems`, short of the shared problems list in
  [core.md](core.md#problem-reporting).

## Open questions

- The detected `url: auto` address and the canvas's story lists are kept until
  the server or canvas reloads. Should a Storybook that moves ports, or a
  title that gains stories, be picked up without a reload?

## Verification points

- [server.test.js](../server.test.js) checks catalog import, port discovery
  from package scripts, automatic detection, exact-title lookup with code
  paths, the not-running and not-a-Storybook errors, and export plans.
- [manifest.test.js](../workbench/manifest.test.js) and
  [config.test.js](../config.test.js) check automatic and catalogued
  implementations, start settings, and a story's state from its ID.
  [page-list.test.js](../workbench/page-list.test.js) checks imported titles and their stories.
- [lenses.test.js](../workbench/lenses.test.js) checks story URLs, address
  picks, and channel messages.
  [preview.test.js](../workbench/preview.test.js) checks channel reuse,
  navigation fallback, and fallback reporting.
  [keys.test.js](../workbench/keys.test.js) checks that editor shortcuts are
  accepted only from the active Storybook iframe and its origin.
- [startup.test.js](../startup.test.js) checks start commands, trust, and
  readiness probes.
