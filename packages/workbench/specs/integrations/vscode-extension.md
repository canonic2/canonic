# VS Code extension contract

This spec covers the editor host around the [core workbench](../core.md). Its
implementation is in [extension.js](../../extension/extension.js), [spaces.js](../../server/node/spaces.js), [panel.js](../../extension/panel.js),
[sidebar-view.js](../../extension/sidebar-view.js), [startup.js](../../extension/startup.js),
[server.js](../../server/node/server.js), and the [host](../../server/node/src/modules/host/index.ts) of each space's
[Workbench server](../system/server.md).
The user-facing reference is [The VS Code extension](../../docs/extension.md).

**Status:** activation, trust gating, commands, the sidebar and workbench tab,
spaces with one server each, refresh, implementation startup, process ownership
and logging are implemented. Complete restart restoration is agreed but not
implemented; Copy Canvas URL in remote workspaces is unverified, and added
spaces outside the workspace run under the window's trust. See
[implementation gaps](#implementation-gaps).

## Activation and ownership

- The extension activates when the workspace contains `workbench.yaml`, and
  after startup in every window, so spaces the user added are reachable
  anywhere. A window with no space serves nothing; its commands explain why
  they cannot open a workbench.
- See [Spaces](#spaces) for which folders hold spaces and which one a
  window shows. Each space gets its own loopback server. The extension
  contributes the Workbench sidebar view, and closes every server and helper
  process on disposal. The Workbench activity-bar view is hidden in a window
  with no space.
- The server listens on 127.0.0.1 only, trying ports 3579–3583 and then any
  free port. It serves project files at `/` and packaged workbench files at
  `/_workbench/` on the same origin.
- A newly installed build needs a VS Code window reload before that window
  runs its new extension code, server, or helper processes.

## Workspace trust

The shared [access-boundary model](../system/access-boundaries.md) distinguishes
execution trust from origin checks, renderer isolation and filesystem access.
[Hosting](hosting.md) owns platform capability and update/reload limits.

- The extension does not declare support for untrusted workspaces, so VS Code
  does not run it in Restricted Mode.
- Start commands and TypeScript previews run project code. Each checks trust
  again before running; untrusted, start commands are skipped with a log entry
  and discovered previews are replaced by the problem
  `Workbench previews require a trusted workspace.` The server may scan for
  definition presence to produce that problem, but returns no discovered
  definitions and executes none in an untrusted workspace.

## Commands and settings

| Command | Contract |
| --- | --- |
| **Workbench: Open Canvas** | Opens or reveals the single Workbench tab. |
| **Workbench: Open Canvas in Browser** | Opens the extension's running server in the default browser. |
| **Workbench: Copy Canvas URL** | Copies the running server's workbench URL. |
| **Workbench: Refresh Pages** | Refreshes the canvas and rebuilds the sidebar (see [Refresh](#refresh)). |
| **Workbench: Switch Space…** | Quick pick of the spaces, current one checked, with **Add a space…**; picking one switches and opens the tab. |
| **Workbench: Add Space…** | Folder picker; adds a folder with `workbench.yaml` and switches to it. |
| **Workbench: Show Log** | Shows the **Workbench** output channel. |

Open Canvas in Browser and Copy Canvas URL act on the current space.

The extension contributes one setting, `workbench.trace` (off by default), which
passes `--trace` to each space's server on startup; see
[request tracing](../system/server.md#request-tracing).

**Open Canvas in Browser** and `node server/node/server.js <project>` serve the same
canvas but differ in host. The browser command uses the extension's server, so
handoffs copy to the VS Code clipboard, source actions open files in the
editor, start commands and trust checks have already applied, and diagnostics
go to the output channel. The standalone server has no handoff (the handoff
route answers `available: false`), no source opening, no start commands, no
trust gate, and no log sink. Both show the same problems list as the sidebar
([core](../core.md#problem-reporting)).

## Editor surfaces and messages

- Selecting the Workbench view in the activity bar opens the workbench tab, or
  brings it forward, and leaves keyboard focus in the page list. Closing the tab
  while the view stays visible leaves it closed until the view is selected
  again.
- The tab opens before implementation readiness and remote port forwarding
  finish, showing a themed **Opening Workbench** state. Server failure replaces
  it with recovery instructions that point to the log. Delayed startup must
  not reopen a tab the user closed, and the latest page pick wins during
  startup.
- The workbench tab hosts one Workbench application document. A content renderer
  inside its canvas never loads that application again; see [rendering flow](../system/rendering-flow.md).
  The tab is a webview containing the hosted workbench in an
  iframe, addressed through `asExternalUri`. The panel relays sidebar picks as
  `wb-go` and settled canvas selections as `wb-here`. It holds a pick until
  the iframe has reported `wb-here` and until any config refresh is
  acknowledged. The sidebar follows navigation initiated in the canvas,
  including links followed in a live page. Chats in the editor don't see
  the tab as context; [agent context](agent-context.md) covers how they learn
  which page it shows.
- Every completed route reports `wb-here`, including a docs page whose rendered
  preview is reused for an example selection or after a config refresh. Reusing
  a preview must still release sidebar picks waiting for that refresh. The
  regression in `extension/panel.test.js` exercises this canvas path through the panel relay.
- Source actions ask the extension to open a file in the editor or reveal a
  directory; the server only opens paths the resolved config names. A handoff
  asks it to copy the generated prompt to the clipboard; the server has
  already saved the screenshot.

## Spaces

The code is [spaces.rs](../../server/src/spaces.rs), which lists and describes
spaces for `workbench-server spaces <folder>…`, [spaces.js](../../server/node/spaces.js),
which keeps the list and the running servers, [extension.js](../../extension/extension.js),
and the [space switcher element](../../ui/src/components/space-switcher/element.js).
Component and theme contracts: [Web Components](../ui/web-components.md) and
[themes](../ui/themes.md). User guide:
[Several spaces](../../docs/spaces.md). Space fields and how pages are
arranged within a space are in the [page tree](../pages/page-tree.md). The
[multiple-artboard canvas](../canvas/multiple-artboards.md#spaces-and-host-transport)
specifies how one canvas keeps artboard instances from several spaces; the
space switcher here changes the editor's host space.

- Spaces come from `workbench.yaml` files: one space per file, or one per
  valid `spaces` entry, each served from its root, under the
  [page tree's space rules](../pages/page-tree.md#spaces). Folders are the window's that have a file, in folder
  order, then the folders the user added (stored in `globalState`, shared by
  every window). A folder in both is listed once, as the window's, and is not
  removable. An added folder whose `workbench.yaml` is missing is hidden but
  stays stored. Files are reread on every listing, so spaces added,
  removed, or re-rooted in the file show up without a reload; a running
  server whose space left the file, or whose root changed, stops.
- A space's id is the first 10 hex digits of the SHA-1 of its file's
  resolved folder, or of `<folder>#<key>` for a space under `spaces`. Its
  name is the manifest `name`, else the folder name (single space) or a
  label from its key, and the folder name when the manifest does not parse.
- A server for a space that isn't its file's whole config, or whose root
  isn't the file's folder, is started with `config: { dir, key }`. It reads
  that space's config, serves the file and its local override at
  `/_workbench/manifest/`, and injects `canonic-config` and `canonic-space`
  metas into the canvas page; the sidebar gets the same metas from the
  extension and watches the file's folder. **Configure pages** writes
  `spaces.<key>.collections` in place, adding it to the entry when the space
  showed the shared collections. Agents: spaces sharing a root share its
  `server.json`; selecting a space re-announces its server there.
- Removal acts on the added folder, so it removes every space its file
  lists. Its mark takes the space's `color` and `icon`
  ([page tree](../pages/page-tree.md#spaces)). Images travel as data URIs,
  because every surface draws every space's mark and none can read another
  space's folder.
- The sidebar rereads the list on every build, and the canvas on every config
  load or refresh, so an edited name, color, or icon shows without a reload.
- Sidebar and canvas icons use the complete pinned Lucide library bundled in
  the extension. The sidebar loads it through an explicit webview resource URL;
  the server serves the same local dependency for the canvas. Icon rendering
  requires that library and reports a loading error if it is absent. No network
  download or separately maintained SVG set supplies icons.
- One space is current per window. The window remembers it in
  `workspaceState` by path; otherwise, and when the remembered one leaves the
  list, the first listed space is current.
- One server per space, started on first open and kept until the space
  leaves the list or the extension stops. A window's own current space
  starts at activation (the single-space behavior); an added space starts
  when the view or canvas opens. A failed start is forgotten so the next open
  retries. All servers share one capture service, closed after them.
- Switching retargets the tab to the new server through the loading state and
  rebuilds the sidebar against the new root, watcher included. Each space's
  last selection is saved independently and restored when returning under the
  [persistence contract](../canvas/persistence.md). Switching is possible from the sidebar's
  switcher, the canvas breadcrumb (relayed as `wb-space` through the panel
  wrapper), and **Switch Space…**.
- **Add a space…** refuses a folder without `workbench.yaml`. Removing an
  added space deletes it from `globalState` and stops its server; if it was
  current, the first space becomes current.
- The list is reread when the window's folders change and when the window
  gains focus, because `globalState` changes made in another window raise no
  event.
- The server answers `GET /_workbench/spaces` with `{ current, spaces }`;
  a server started without a list reports only itself. `POST
  /_workbench/spaces/open` with `{ id }` answers the space's workbench URL,
  starting it if needed. It is refused (403) for a request whose `Origin` is
  not the server's own or whose `Sec-Fetch-Site` is cross-site or same-site,
  since starting a space can run its start commands.
- The canvas shows the breadcrumb space and, outside the editor, a sidebar
  switcher only with two or more spaces. The editor's sidebar always shows
  the switcher, as the home of **Add a space…**.
- `node server/node/server.js a b …` starts every space eagerly, shares one capture
  service, and prints one `workbench on <url>  <name>` line per space. One
  folder produces one server and one reported URL.

### Persistence and restored editor tabs

The extension's `workspaceState` storage, restored editor tabs and their
lifecycle follow [persistence and restoration](../canvas/persistence.md#vs-code-ownership-and-lifecycle),
which owns them.

## Sidebar and problems

- The Workbench sidebar view is an extension webview that reads the YAML
  itself and uses the same navigation model as the canvas.
- When previews are enabled or any implementation has a catalog, it asks the
  extension for the server's imported collections and `problems` before building.
  Until they arrive it shows **Waiting for** the pending catalogs, or
  **Finding previews…**, and disables search.
- Problems from that answer are listed above the page list. If the request
  itself fails, that failure is listed instead and authored pages still
  build.
- A YAML file the sidebar cannot read replaces the page list with its error.

## Refresh

- The extension watches `workbench.yaml` and `workbench.local.yaml`. The
  server's resident index watches definitions matching the resolved discovery
  patterns (`previews.include`, or the defaults). Adding, removing, or
  renaming a definition, or changing its title, scenarios, or sizes, updates
  the sidebar and canvas without **Refresh Pages**. With `previews: false`,
  only the YAML files are watched. **Refresh Pages** runs the same rebuild
  by hand.
- A rebuild first asks an open canvas to reload its config, then rebuilds the
  sidebar, which asks the server again and so re-runs preview discovery and
  catalog imports. Picks from the new sidebar wait until the canvas has
  acknowledged the refresh, so a pick of a new page or scenario does not target
  an old manifest. A refresh rejected for an invalid manifest keeps the
  queued pick until a later refresh succeeds.
- The running pipeline watches preview source dependencies and pushes build
  results. The resident index reports changed definition metadata through the
  host so it runs the same rebuild. See [the server watcher](../system/server.md#the-resident-index-and-watcher).

## Implementation startup

- In a trusted workspace, each URL or Storybook implementation may define
  `start.command`, `start.cwd`, `start.check`, `start.ready`, and
  `start.timeout` (1–300 seconds, default 60). The extension checks the
  configured TCP port or HTTP URL first. If it is already available, it leaves
  the terminal alone. Otherwise it opens a VS Code terminal in the configured
  directory and runs the command. Implementations start in parallel, and the
  workbench server starts once each is ready or has timed out.
- Startup failures and timeouts are logged and do not prevent the server from
  starting; the affected preview or catalog reports its own problem. The
  standalone server and browser do not run start commands.

## Process boundaries

The extension host starts and owns these processes; the
[server's processes and connections](../system/server.md#processes-and-connections)
own the server, its children and how each is reached:

- **A host per space**, which starts that space's `workbench-server` and
  answers it on stdin and stdout for the editor, capture and spaces. On macOS
  it also runs the window capture helper, a ScreenCaptureKit stream of a
  Simulator or app window for the
  [Simulator and window lenses](../pages/implementations.md#native-window-stream),
  a separate process and Screen Recording permission path.
- **One screenshot capture service**, the bundled Electron helper shared by
  every space, which renders pages for [interactive capture](../review/capture.md)
  and [export](../review/export.md). Its runtime, unpacked once into the
  extension's global storage, is specified in
  [capture](../review/capture.md#bundled-electron-runtime).
- **VS Code webviews**: the sidebar and the workbench tab.

Browser-only VS Code cannot spawn any of these local processes.

## Logging

Server, canvas, capture, preview compiler, and handoff diagnostics go to the
**Workbench** output channel as bounded event records. Each server stops
logging after 2 MB per session. Records never contain screenshots, page HTML,
or handoff prompts.

## Ownership constraints

- Each space has its own server, origin, configuration, preview compiler and
  agent-view state. Servers start lazily where possible and share the host's
  capture service.
- Added-space membership is global to the editor installation. Current-space
  selection is per window.
- Discovery and refresh must preserve the latest selection and named problems.

## Verification points

- [panel.test.js](../../extension/panel.test.js) checks the immediate loading tab, a closed
  loading tab staying closed, failed-startup recovery text, readiness when
  `wb-here` precedes iframe load, pick resends until acknowledged, and refresh
  ordering, including a refresh rejected for an invalid manifest.
- [sidebar-view.test.js](../../extension/sidebar-view.test.js) checks that catalog problems and
  catalog request failures reach the sidebar, and that showing the view opens
  the canvas.
- [sidebar.test.cjs](../../ui/src/modules/project/sidebar.test.cjs) checks the waiting text,
  disabled search, the problems list, preview-only catalog requests, and the
  immediate build when previews are off.
- [spaces.test.js](../../server/node/spaces.test.js) checks naming and fallback, ids,
  deduplication, lazy and single starts, retry after a failed start, and
  stopping on removal and close. [server.test.js](../../server/node/server.test.js) checks
  the `/_workbench/spaces` routes, on-demand start through another server,
  and the cross-origin refusal. [panel.test.js](../../extension/panel.test.js) checks
  retargeting an open tab, a slower earlier load not winning, a closed tab
  staying closed, and `wb-space` relay. [sidebar-view.test.js](../../extension/sidebar-view.test.js)
  and [sidebar.test.cjs](../../ui/src/modules/project/sidebar.test.cjs) check the
  `canonic-spaces` meta, watcher and resource retargeting, and the
  switcher's messages.
- [startup.test.js](../../extension/startup.test.js) checks probes, terminal startup only
  when the check fails, and skipping in untrusted workspaces.
- [server.test.js](../../server/node/server.test.js) checks that only config-resolved paths
  open and that handoff is advertised only when the editor provides it.
- [preview.test.js](../../server/node/preview.test.js) checks that the server starts, serves
  through, and closes previews, stops serving after `previews: false`, and
  never runs definitions when untrusted or disabled.
- Manual, not automated: Node evaluator restart after it is killed, compiler
  replacement on a `previews` change, the YAML watcher, the sidebar and canvas updating after
  a definition is added (the server watcher's scope rules have unit tests in
  `server/src/serve/resident.rs`; the editor rebuild has none), activity-bar visibility without `workbench.yaml`, handoff
  from **Open Canvas in Browser**, and in VS Code: adding, switching, and
  removing spaces, the window-focus reread, and extension.js wiring. The
  canvas and sidebar switchers also have browser fixture coverage; that does
  not establish editor-window or remote-host acceptance.

## Implementation gaps

- **Restart restoration is incomplete.** Complete per-space selection and
  preference restoration (preferences live in browser `localStorage`) and
  restored editor tabs do not follow the
  [persistence contract](../canvas/persistence.md#implementation-gaps); current-space
  storage, hidden-tab retention and first-page fallback do.
- **Copy Canvas URL in remote workspaces (unverified).** The command copies
  the server's `127.0.0.1` address without `asExternalUri`; in a remote
  workspace that names the remote machine. The tab uses `asExternalUri`, and
  **Open Canvas in Browser** relies on `openExternal`.
- **Added spaces and workspace trust.** VS Code trust covers the open
  workspace only. An added space outside it runs start commands and
  previews under the window's trust flag; the guide tells users to add only
  folders they trust.

## Open questions

- **Restricted Mode and live trust.** Decide whether Workbench should expose
  non-executing inspection in untrusted workspaces. The extension
  declares no Restricted Mode support, and its server trust flag is fixed at
  creation. Supporting that workflow requires a live trust contract for both
  workspace and added-space roots; start commands and evaluation must remain
  gated throughout transitions.
