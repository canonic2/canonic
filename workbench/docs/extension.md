# The VS Code extension

The Workbench extension runs a local server for each space, shows the
sidebar in the Workbench view and the canvas in an editor tab, and connects
handoffs and source links to the editor. This page is the reference for its
commands and processes.

## Activation

The extension serves an open folder that contains `workbench.yaml` at its
root. The file is one space, or several when it lists
[`spaces`](configuration.md#spaces). In a multi-root workspace, every
folder with a `workbench.yaml` adds its spaces, and the first one shows
unless the window last showed another; see [Several spaces](spaces.md). A window without one gets no server and no
Workbench view, unless you have [added spaces](spaces.md#add-a-space).
The commands stay available, and say why they can't run.

Selecting Workbench or running **Workbench: Open Canvas** opens the tab
immediately. It shows **Opening Workbench** while configured services, such as
Storybook, start. The sidebar shows which catalogs it is waiting for, or
**Finding previews…** during preview discovery. Search becomes available when
the list is ready. The canvas shows **Loading pages…** while reading its
catalogs, and **Loading preview…** for the first preview. Later page changes
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

### Several spaces

One window can switch between several spaces: those described by the
`workbench.yaml` of the window's folders, which can list several each, and of
folders you add from elsewhere on disk. Each space has its own server and
port. See [Several spaces](spaces.md).

## Commands

| Command | Does |
| --- | --- |
| **Workbench: Open Canvas** | Opens the canvas in an editor tab. |
| **Workbench: Open Canvas in Browser** | Opens the same canvas in your default browser. |
| **Workbench: Copy Canvas URL** | Copies the canvas address, for a bookmark, a script, or an agent. |
| **Workbench: Refresh Pages** | Re-reads `workbench.yaml` and `workbench.local.yaml`, finds [TypeScript previews](workbench-previews.md) and [`defineDocs`](docs-pages.md#declare-a-docs-page-in-a-definition) docs pages again, and refreshes the sidebar and canvas. This happens automatically when either YAML file changes. Run it after you add or remove a `*.workbench.ts` file. |
| **Workbench: Switch Space…** | Lists the [spaces](spaces.md) and shows the one you pick. |
| **Workbench: Add Space…** | Adds a folder with a `workbench.yaml` to the spaces and switches to it. |
| **Workbench: Show Log** | Opens the Workbench output log. |

**Open Canvas in Browser** and **Copy Canvas URL** use the current space.

## The server

Each space gets its own server:

- It listens on **127.0.0.1 only**. It serves your project folder, so nothing
  outside your machine can reach it.
- It uses port **3579**, then 3580 to 3583, then any free port the system
  offers. With several windows or spaces running, each space gets the next
  free port, in the order the servers started.
- It serves the project at `/` and the workbench at `/_workbench/` on the same
  origin, which is what lets the workbench read and capture the live page.
- It adds one `preview-compat.js` bundle to served HTML and managed previews. See
  [How pages are served](pages-and-states.md#how-pages-are-served).

**Workbench: Copy Canvas URL** gives the actual address.

## The screenshot helper

Packaged builds include a screenshot helper built on Electron. It starts with
the first space's server, is shared by every space in the window, and
stays running until the extension stops, so screenshots are fast:

- It keeps an inert, scriptless copy of the page you're viewing up to date in
  the background. See [How screenshots are taken](annotations-and-handoff.md#how-screenshots-are-taken).
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

When the space has [TypeScript previews](workbench-previews.md) or
[docs pages](docs-pages.md) with examples, the server starts a separate worker
process that compiles them and runs their project code: definitions, examples,
`workbench.config.ts`, compiler plugins, and Astro rendering. Packaged builds run it on the bundled runtime, so it doesn't need a
separate Node installation.

- It starts the first time previews are listed and stops with the server.
- If it stops, the next preview request starts a new one.
- Changing the `previews` key in `workbench.yaml` restarts it with the new
  settings. With `previews: false`, nothing is compiled and no worker runs:
  docs pages show their Markdown, and each example panel says why its example
  is missing.

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
[TypeScript previews](workbench-previews.md) and
[docs page](docs-pages.md) examples, which run project code.

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
  [URL implementations](lenses.md), and Storybook. Implementations load
  through a proxy on `127.0.0.1` that adds the
  [preview bridge](lenses.md#screenshots-through-a-url-lens) to their pages.
  If the bridge hasn't connected, the screenshot helper loads a lens's URL
  itself to capture it.
- `url: auto` probes local ports for a running Storybook, and start commands
  probe the `check` and `ready` addresses you configure.
- A handoff goes only to your clipboard and `.canonic/.handoffs/`. Nothing is
  sent to an agent until you paste it.

## Updating

An install from the Visual Studio Marketplace updates like any other
extension. For an install from a `.vsix`, download the newer `.vsix` from the
[latest release](https://github.com/canonic2/canonic/releases/latest) and
install it the same way as the first one; it replaces the installed version.
To switch a `.vsix` install in VS Code to Marketplace updates, uninstall it
and install **Canonic Workbench** from the Extensions view.

Then run **Developer: Reload Window** in each open project window. Installing
replaces the files on disk, but running windows, servers, screenshot helpers,
and preview workers keep the old code until their window reloads. Reloading
the canvas or running **Refresh Pages** isn't enough.

## Uninstalling

Uninstall **Canonic Workbench** from the Extensions view, or from the terminal:

```sh
code --uninstall-extension canonic.canonic-workbench
```

Then reload any window that had a project open, to stop its server and
helpers. Uninstalling leaves your projects as they are; to remove Workbench's
files from a project too, see
[Removing Workbench from a project](getting-started.md#removing-workbench-from-a-project).

## Running without VS Code

The extension's server runs on its own with Node 24 or later. Find the
installed extension's folder and point the server at a project:

```sh
node ~/.vscode/extensions/canonic.canonic-workbench-*/server.js path/to/project
```

It prints `workbench on http://127.0.0.1:3579/_workbench/` (or the port it
got). Open that address in a browser. Without a project argument, it serves
the current folder. Give it several folders to switch between their spaces in
the browser; see [Several spaces](spaces.md#in-a-browser). Stop it with
Ctrl+C.

The folder name ends in the version and platform, such as
`canonic.canonic-workbench-0.7.0-darwin-arm64`. On Windows, extensions are in
`%USERPROFILE%\.vscode\extensions`. Cursor, Windsurf, and other forks keep
extensions in their own folder, such as `~/.cursor/extensions`.

What works where:

| | VS Code, or **Open Canvas in Browser** | `server.js` in a browser |
| --- | --- | --- |
| Pages, states, sizes, zoom | Yes | Yes |
| Lenses, including Simulator and window streams | Yes | Yes |
| TypeScript previews | Yes | Yes |
| Docs pages and their examples | Yes | Yes |
| Annotations and camera | Yes | Yes |
| Handoff | Yes | No |
| Open source files in the editor | Yes | No |
| Start commands | Yes | No |
| Design-system export | Yes | Yes |
| Switch spaces | Yes; add and remove in VS Code | Between the folders it was given |
| Problems list | VS Code's sidebar only | No; read the [config route](troubleshooting.md#read-the-resolved-config) |

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
