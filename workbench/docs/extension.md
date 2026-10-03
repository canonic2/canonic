# The VS Code extension

The Workbench extension runs a local server for each project, shows the screen
list in the sidebar and the canvas in an editor tab, and connects handoffs and
source links to the editor. This page is the reference for its commands
and processes.

## Activation

The extension activates when an open folder contains `workbench.yaml` at its
root. In a multi-root workspace, the first folder with a `workbench.yaml` is
served. A folder without one gets no server and no Workbench view. The
commands stay available, and say why they can't run.

Selecting Workbench or running **Workbench: Open Canvas** opens the tab
immediately. It shows **Opening Workbench** while configured services, such as
Storybook, start. The sidebar shows which catalogs it is waiting for, or
**Finding previews…** during preview discovery. Search becomes available when
the list is ready. The canvas shows **Loading screens…** while reading its
catalogs, and **Loading preview…** for the first preview. Later screen changes
keep the current preview visible until its replacement is ready.

If the server cannot start, the tab says **Workbench couldn't start** and
points to the Workbench log; reload the window to retry. Closing a loading tab
keeps it closed when startup finishes.

### Projects in a subfolder

Workbench looks only at the top of each open folder, not inside it. When
`workbench.yaml` lives in a subfolder, such as `apps/design` in a monorepo,
either:

- open that subfolder as its own window, or
- add it to the workspace with **File** › **Add Folder to Workspace…**, so the
  rest of the repository stays open beside it.

The folder that holds `workbench.yaml` is the project root: every `src`,
`root`, and `cwd` in it is relative to that folder, and the server serves only
that folder. Implementation code elsewhere in the repository is reached
through an implementation's [`root`](configuration.md#implementations), such as
`root: ../../packages/ui`.

### Several projects

Each VS Code window serves its own project, with its own server, screenshot
helper, and port. Open each project in its own window to work on several at
once. One window serves only one workbench, so a multi-root workspace with two
`workbench.yaml` files shows the first.

## Commands

| Command | Does |
| --- | --- |
| **Workbench: Open Canvas** | Opens the canvas in an editor tab. |
| **Workbench: Open Canvas in Browser** | Opens the same canvas in your default browser. |
| **Workbench: Copy Canvas URL** | Copies the canvas address, for a bookmark, a script, or an agent. |
| **Workbench: Refresh Screens** | Re-reads `workbench.yaml` and `workbench.local.yaml`, finds [TypeScript previews](workbench-previews.md) again, and refreshes the sidebar and canvas. This happens automatically when either YAML file changes. Run it after you add or remove a preview file. |
| **Workbench: Show Log** | Opens the Workbench output log. |

## The server

Each project gets its own server:

- It listens on **127.0.0.1 only**. It serves your project folder, so nothing
  outside your machine can reach it.
- It uses port **3579**, then 3580 to 3583, then any free port the system
  offers. With several windows open, each project gets the next free port, in
  the order the windows started.
- It serves the project at `/` and the workbench at `/_workbench/` on the same
  origin, which is what lets the workbench read and capture the live page.
- It adds one `preview-compat.js` bundle to served HTML and managed previews. See
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
- If it crashes, it restarts and restores the latest view, waiting longer
  between attempts while it keeps failing. If a page hangs it, the helper is
  replaced, and the page is loaded again the next time the canvas asks for it.

Screenshots need this helper. It runs on macOS 13 or later, Windows, and Linux
with a display. Elsewhere, or if it can't be unpacked or started, the camera
and **Copy handoff** show the reason instead of a screenshot, and a
design-system export lists the missing reference screenshots as warnings.

The [iOS Simulator](ios-simulator.md) and [app window](windows.md) streams use
a separate native helper and need Screen Recording permission.

## The preview worker

When the project has [TypeScript previews](workbench-previews.md), the server
starts a separate worker process that compiles them and runs their project
code: definitions, `workbench.config.ts`, compiler plugins, and Astro
rendering. Packaged builds run it on the bundled runtime, so it doesn't need a
separate Node installation.

- It starts the first time previews are listed and stops with the server.
- If it stops, the next preview request starts a new one.
- Changing the `previews` key in `workbench.yaml` restarts it with the new
  settings. With `previews: false`, nothing is compiled and no worker runs.

## Logs

Errors from the canvas, the screenshot helper, the preview worker, the server,
and handoffs are written to the **Workbench** output log. Run **Workbench: Show Log** to open it.

- VS Code manages the log and its retention. Nothing is written to your project.
- Each server stops logging after 2 MB in a session.
- Entries contain event details and errors only, never screenshots, page HTML,
  or handoff prompts.

## Workspace trust

Workbench doesn't declare support for untrusted workspaces, so VS Code doesn't
run it in Restricted Mode. Trust the folder to use it. Trusting a folder lets
Workbench run [start commands](configuration.md#start-commands), which are
shell commands from `workbench.yaml`, and
[TypeScript previews](workbench-previews.md), which run project code.

## Files and network access

In your project, Workbench writes only:

- `.canonic/.handoffs/`, the screenshots that **Copy handoff** saves. Add it to
  `.gitignore`.
- `workbench.yaml`, when you save **Configure pages**.

Outside the project:

- The camera's screenshots go to your browser's or editor's downloads folder.
- The bundled runtime and the native window helper are unpacked into the
  extension's storage in your editor's user data folder (`globalStorage`).
  A standalone server uses the system's temporary folder instead.
- The log is kept by VS Code. See [Logs](#logs).

Network access:

- The server listens on `127.0.0.1` only, so other machines can't reach it.
- The extension sends no telemetry and downloads nothing at runtime. The
  runtime it needs ships inside the `.vsix`.
- The canvas loads what you point it at: your own pages and their assets,
  [URL implementations](lenses.md), and Storybook. Without the
  [preview bridge](lenses.md#screenshots-through-a-url-lens), the screenshot
  helper loads a lens's URL itself to capture it.
- `url: auto` probes local ports for a running Storybook, and start commands
  probe the `check` and `ready` addresses you configure.
- A handoff goes only to your clipboard and `.canonic/.handoffs/`. Nothing is
  sent to an agent until you paste it.

## Updating

Workbench isn't installed from the Marketplace, so it doesn't update itself.
Download the newer `.vsix` from the
[latest release](https://github.com/canonic2/canonic/releases/latest) and
install it the same way as the first one; it replaces the installed version.

Then run **Developer: Reload Window** in each open project window. Installing
replaces the files on disk, but running windows, servers, screenshot helpers,
and preview workers keep the old code until their window reloads. Reloading
the canvas or running **Refresh Screens** isn't enough.

## Uninstalling

Uninstall **Workbench** from the Extensions view, or from the terminal:

```sh
code --uninstall-extension canonic.canonic-workbench
```

Then reload any window that had a project open, to stop its server and
helpers. Uninstalling leaves your projects as they are; to remove Workbench's
files from a project too, see
[Removing Workbench from a project](getting-started.md#removing-workbench-from-a-project).

## Running without VS Code

The extension's server runs on its own with Node 18 or later. Find the
installed extension's folder and point the server at a project:

```sh
node ~/.vscode/extensions/canonic.canonic-workbench-*/server.js path/to/project
```

It prints `workbench on http://127.0.0.1:3579/_workbench/` (or the port it
got). Open that address in a browser. Without a project argument, it serves
the current folder. Stop it with Ctrl+C.

The folder name ends in the version and platform, such as
`canonic.canonic-workbench-0.7.0-darwin-arm64`. On Windows, extensions are in
`%USERPROFILE%\.vscode\extensions`. Cursor, Windsurf, and other forks keep
extensions in their own folder, such as `~/.cursor/extensions`.

What works where:

| | VS Code, or **Open Canvas in Browser** | `server.js` in a browser |
| --- | --- | --- |
| Screens, states, widths, zoom | Yes | Yes |
| Lenses, including Simulator and window streams | Yes | Yes |
| TypeScript previews | Yes | Yes |
| Markup and camera | Yes | Yes |
| Handoff | Yes | No |
| Open source files in the editor | Yes | No |
| Start commands | Yes | No |
| Design-system export | Yes | Yes |
| Problems listed above the screens | VS Code's screen list only | No; read the [config route](troubleshooting.md#read-the-resolved-config) |

## Platform support

Desktop macOS is the most exercised platform. Windows and Linux builds, remote
workspaces (SSH, containers, WSL), and other platforms have had less
validation. A remote workspace runs the extension on the remote machine, so
install the `.vsix` built for that machine's platform, and on a Linux host
without a display, screenshots aren't available. Browser-only VS Code, such as
vscode.dev, can't run the local server or helper. The
[iOS Simulator](ios-simulator.md) and [app window](windows.md) lenses need
macOS.

Release builds are unsigned.
