# Storybook contract

Storybook is an opt-in implementation. A project can map an authored screen to
one exact Storybook title, import a whole Storybook catalog, or do both.
See [manifest.js](../../workbench/manifest.js), [server.js](../../server.js),
[lenses.js](../../workbench/lenses.js), and
[workbench.js](../../workbench/workbench.js).

## Configuration

```yaml
implementations:
  storybook:
    kind: storybook
    url: http://localhost:6006 # or auto
    root: packages/ui          # optional, for source links
    catalog: true              # optional; may be a map of icon settings
    start:                     # optional in VS Code
      command: yarn storybook
      cwd: .
      check:
        port: 6006
      ready:
        url: http://localhost:6006/index.json
      timeout: 90
```

- An explicit `url` is an HTTP(S) Storybook origin. `url: auto` checks ports
  found in project package scripts and ports 6006–6010 for a live
  `/index.json`. Detection never launches a process by itself.
- `start` may launch a command in a trusted VS Code workspace. Workbench first
  runs `check` (TCP `port` with optional `host`, or an HTTP(S) `url`). It opens
  a terminal and executes `command` in `cwd` only when that check fails. It
  then waits for `ready`, or `check` if `ready` is omitted, up to `timeout`
  seconds. A check that is already passing does not open another terminal.
- `catalog: true` imports the Storybook index. A catalog map may give a
  fallback `icon` and title-prefix `icons`; the longest matching prefix wins.
  Without `catalog`, only explicitly mapped authored screens use Storybook.

## Catalog and story identity

- Workbench reads Storybook's live `/index.json`. Only entries with
  `type: story`, an ID, and a title become selectable stories. Entries are
  grouped by their exact `title`.
- For a title such as `UI/Components/Button`, `UI` becomes a section,
  `Components` becomes a folder, and `Button` becomes a screen. Titles with
  multiple stories expand into state rows; a one-story title stays one row.
  An imported screen has a synthetic `__storybook/…`
  address and no design page or Design lens.
- Selecting a title's story preserves its Storybook ID. The suffix after
  `--` is the workbench state in the address. The preview URL is
  `<url>/iframe.html?id=<story-id>&viewMode=story`.
- An authored screen can map one implementation to a title, for example
  `storybook: UI/Components/Button`. Story lookup matches that title exactly;
  a similar final segment under another prefix is not silently substituted.
  The story menu lists the title's stories.
- The server fetches stories for the selected title and provides source paths
  from Storybook's `componentPath` and `importPath` when an implementation
  `root` is configured. A failed lookup reports the missing title; a stopped
  Storybook does not remove other usable screens.

## Switching the visible story

- The first story loads in the preview iframe. Later picks on the same
  Storybook origin ask the loaded preview to switch through Storybook's
  `setCurrentStory` channel. This keeps the Storybook runtime warm.
- Workbench accepts `currentStoryWasSet` and `storyRendered` only from the active
  or loading preview iframe, at the configured Storybook origin, and for the
  requested story ID.
  It reports the selection to the VS Code sidebar and signals a new frame to
  capture only after `storyRendered`. A pending selection keeps the editor
  bridge ready for another pick without claiming the old canvas has changed.
- If Storybook does not acknowledge a switch within 1.5 seconds, or accepts
  it but does not report rendering within 15 seconds, Workbench navigates to
  that story's `iframe.html` URL. A newer pick cancels the older fallback.
  An initial load, a pending iframe navigation, or a different Storybook
  origin also uses iframe navigation.

## Export and verification

- Design-system export plans a reference for each imported story at the
  supported viewport sizes. [Export capture](export.md) schedules background
  workers and does not drive the visible workbench preview.
- [server.test.js](../../server.test.js) checks catalog import, exact-title
  lookup, and export plans. [lenses.test.js](../../workbench/lenses.test.js)
  checks story URLs and channel messages.
  [preview.test.js](../../workbench/preview.test.js) checks reuse and fallback.
