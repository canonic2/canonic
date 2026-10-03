# The VS Code extension

The Workbench extension runs a local server for each project, shows the screen
list in the sidebar and the canvas in an editor tab, and connects handoffs and
source links to the editor. This page is the reference for its commands,
settings, and processes.

## Activation

The extension activates when an open folder contains `workbench.yaml` at its
root. In a multi-root workspace, the first folder with a `workbench.yaml` is
served. A folder without one gets no server and no Workbench view. The
commands stay available, and say why they can't run.

## Commands

| Command | Does |
| --- | --- |
| **Workbench: Open Canvas** | Opens the canvas in an editor tab. |
| **Workbench: Open Canvas in Browser** | Opens the same canvas in your default browser. |
| **Workbench: Copy Canvas URL** | Copies the canvas address, for a bookmark, a script, or an agent. |
| **Workbench: Refresh Screens** | Re-reads `workbench.yaml` and `workbench.local.yaml`, and refreshes the sidebar and canvas. This happens automatically when either file changes. |
| **Workbench: Show Log** | Opens the Workbench output log. |

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `canonic.capture.chromePath` | empty | A Chrome, Chromium, or Edge executable for fallback screenshots. Used only when the bundled screenshot helper is missing or can't start. Leave it empty to find Chrome in its standard location. |

## The server

Each project gets its own server:

- It listens on **127.0.0.1 only**. It serves your project folder, so nothing
  outside your machine can reach it.
- It uses port **3579**, then 3580 to 3583, then any free port the system
  offers.
- It serves the project at `/` and the workbench at `/_workbench/` on the same
  origin, which is what lets the workbench read and capture the live page.
- It adds `actions.js` and `states.js` to the HTML pages it serves. See
  [How pages are served](pages-and-states.md#how-pages-are-served).

**Workbench: Copy Canvas URL** gives the actual address.

## The screenshot helper

Packaged builds include a screenshot helper built on Electron. It starts with
the project's server and stays running until the extension stops, so
screenshots are fast:

- It keeps an inert, scriptless copy of the page you're viewing up to date in
  the background. See [How screenshots are taken](markup-and-handoff.md#how-screenshots-are-taken).
- On macOS it runs as a background agent, with no Dock icon or window.
- On first activation the runtime is unpacked into the extension's storage.
  Later activations reuse it, and runtimes from older versions are removed.
- If it crashes, it restarts and restores the latest view. A page that keeps
  hanging it is retried with a growing delay.

If the helper can't be unpacked or started, or on a Linux machine without a
display, Workbench takes screenshots with headless Chrome, Chromium, or Edge
instead, and keeps using it until the server stops. Set
`canonic.capture.chromePath` if the browser isn't in its standard location.

The [iOS Simulator](ios-simulator.md) stream uses a separate native helper and
needs Screen Recording permission.

## Logs

Errors from the canvas, the screenshot helper, the server, and handoffs are
written to the **Workbench** output log. Run **Workbench: Show Log** to open it.

- VS Code manages the log and its retention. Nothing is written to your project.
- Each server stops logging after 2 MB in a session.
- Entries contain event details and errors only, never screenshots, page HTML,
  or handoff prompts.

## Workspace trust

Workbench doesn't declare support for untrusted workspaces, so VS Code doesn't
run it in Restricted Mode. Trust the folder to use it. [Start commands](configuration.md#start-commands),
which run shell commands from `workbench.yaml`, check trust again before they
run.

## Updating

After installing a new version, run **Developer: Reload Window** in each open
project window. Installing replaces the files on disk, but running windows,
servers, and screenshot helpers keep the old code until their window reloads.
Reloading the canvas or running **Refresh Screens** isn't enough.

## Running without VS Code

The extension's server runs on its own with Node 18 or later. Find the
installed extension's folder and point the server at a project:

```sh
node ~/.vscode/extensions/canonic.canonic-workbench-*/server.js path/to/project
```

It prints `workbench on http://127.0.0.1:3579/_workbench/` (or the port it
got). Open that address in a browser. Cursor, Windsurf, and other forks keep
extensions in their own folder, such as `~/.cursor/extensions`.

What works where:

| | VS Code | `server.js` in a browser | Page opened from disk |
| --- | --- | --- | --- |
| Screens, states, widths, zoom | Yes | Yes | Needs `--allow-file-access-from-files` |
| Lenses | All | All | Frame only, no Storybook lookup |
| Markup and camera | Yes, downloads a JPEG | Yes, downloads a JPEG | Yes, slower |
| Handoff | Yes | No | No |
| Open source files in the editor | Yes | No | No |
| Start commands | Yes, in trusted workspaces | No | No |
| Design-system export | Yes | Yes | No |

## Platform support

Desktop macOS is the most exercised platform. Windows and Linux builds, remote
workspaces (SSH, containers, WSL), and other platforms have had less
validation. Browser-only VS Code, such as vscode.dev, can't run the local
server or helper.

Release builds are unsigned.
