# Storybook adapter

Part of the [adapters](README.md). User guide: [Storybook](../../docs/storybook.md).
The [Storybook contract](../storybook.md) owns configuration, story identity,
story switching, screenshots and export; this spec owns how the Workbench
server gets Storybook to render a story.

## Source

A page maps to one exact Storybook title, and its stories are the page's
states. Storybook renders them with the project's own Storybook, so its
addons, decorators, loaders, `preview.ts` and builder configuration apply.

## How a story is served

Workbench uses two parts of Storybook: `/index.json` for the titles and
stories, and `iframe.html?id=<story>&viewMode=story` for rendering one. It
never loads Storybook's manager interface. The canvas frames stories
through the Workbench server's
[implementation proxy](../implementation-proxy.md), never from Storybook's
own address.

## Target

Proposed:

- **The Workbench server runs the project's Storybook.** With only `root`
  set, it starts the project's installed `storybook dev` with
  `--preview-only` (no manager build), `-h 127.0.0.1`, a port it picks with
  `--exact-port`, and `--ci --no-open --quiet --no-version-updates
  --disable-telemetry`. It starts it when a page first needs a story,
  treats it as ready when `/index.json` answers, restarts it on a crash, and
  stops it with the server.
- **A loopback port stays.** Storybook listens only on TCP, and its hot
  updates (Vite's WebSocket, webpack's event stream) use the same server.
  Driving Storybook's internal Node API without a port would break between
  versions.
- `url` remains for a Storybook the user runs, and `start` for a project's
  own command.

To verify in a lab: that `--preview-only` serves `/index.json` and hot updates
as Workbench uses them, the Storybook versions that have it, and its start
time against a full `storybook dev`.

## Decisions

Decided by the owner on 2026-10-17:

- **Pages are declared in `workbench.yaml`.** Each Storybook page is a page
  of the space mapped to one exact title. Storybook has no catalog import
  that turns every title into a page, so every page of a space comes from
  its own configuration or its definitions.
- **Storybook for React Native is not supported.** React Native components
  render through the [React Native Web adapter](react-native-web.md): less
  exact than a device, and right for designing in the browser beside the
  code.
- **Stories are not rendered with Workbench's own adapters.** Storybook's
  portable stories API would need no Storybook server, but it applies only
  preview annotations, not builder configuration or framework presets.

## Status

Workbench reads a running Storybook, started by the user or by `start` in
VS Code, for the pages mapped to it. The owned preview-only launch is not
implemented.
