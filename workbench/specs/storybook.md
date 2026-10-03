# Storybook contract

Storybook is an opt-in implementation. A project can map an authored screen to
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
  mapped authored screens use Storybook.

## Catalog and story identity

- Only index entries with `type: story`, a string ID, and a title become
  selectable stories; docs entries are skipped. Entries are grouped by their
  exact `title`.
- For a title such as `UI/Forms/Inputs/Text field`, `UI` becomes a section,
  `Forms / Inputs` becomes one folder (folders don't nest), and `Text field`
  becomes a screen. A one-segment title goes in a section named after the
  implementation's label. Each story is one of the screen's states, labeled
  with the story's name; a one-story title stays one row.
- An imported section joins an authored section with the same name. Imported
  screens have a synthetic `__storybook/<implementation>/<component>.html`
  address, open straight on the Storybook lens, and have no Design lens.
- A story's workbench state is the part of its ID after `--`; the address
  stores that state and the story keeps its full Storybook ID. The preview URL
  is `<url>/iframe.html?id=<story-id>&viewMode=story`. **Open on its own**
  opens the same story inside Storybook at `<url>/?path=/story/<story-id>`.
- An authored screen can map one implementation to a title, for example
  `storybook: UI/Components/Button`. Story lookup matches that title exactly;
  a similar final segment under another prefix is not silently substituted.
  When only the prefix differs, the lookup error suggests the matching titles.
- The Storybook lens shows the title's first story, or the one the address
  names. On mapped and imported screens alike, the toolbar's **State** menu
  lists the title's stories in place of the design's states.
- The server fetches stories for the selected title through
  `/_workbench/stories`. The canvas keeps each title's list for the session.
  Source paths come from Storybook's `componentPath` and `importPath`,
  resolved against `root` and kept only when they exist; a screen's own
  `code` pointers are added after them. A failed lookup shows its error on
  the canvas, and a stopped Storybook does not remove other usable screens.
- The Actions switch is disabled and shown as on through a Storybook lens.

## Switching the visible story

- The first story loads in the preview iframe. Later picks on the same
  Storybook origin ask the loaded preview to switch through Storybook's
  `setCurrentStory` channel. This keeps the Storybook runtime warm.
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

- Without the preview bridge, a screenshot of a story is taken by the capture
  helper loading the story URL itself, so interaction state isn't included.
  Loading `/_workbench/preview-bridge.js` from `.storybook/preview` lets the
  workbench capture the live story. See [capture.md](capture.md).
- Design-system export plans a reference for each imported story at each of
  the screen's viewports; imported screens declare none, so they use fit,
  desktop, and mobile. A state without a story ID becomes a warning. An
  authored screen mapped to Storybook exports its design page, not the story.
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
  mapped screens is stopped, the failure appears on the canvas when such a
  screen is opened, not in `problems`, short of the shared problems list in
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
  [nav.test.js](../workbench/nav.test.js) checks imported story rows.
- [lenses.test.js](../workbench/lenses.test.js) checks story URLs, address
  picks, and channel messages.
  [preview.test.js](../workbench/preview.test.js) checks channel reuse,
  navigation fallback, and fallback reporting.
  [keys.test.js](../workbench/keys.test.js) checks that editor shortcuts are
  accepted only from the active Storybook iframe and its origin.
- [startup.test.js](../startup.test.js) checks start commands, trust, and
  readiness probes.
