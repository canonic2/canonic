# VS Code extension contract

This spec covers the editor host around the [core workbench](core.md). Its
implementation is in [extension.js](../extension.js), [spaces.js](../spaces.js), [panel.js](../panel.js),
[sidebar-view.js](../sidebar-view.js), [startup.js](../startup.js),
[server.js](../server.js), and [preview-service.js](../preview-service.js).
The user-facing reference is [The VS Code extension](../docs/extension.md).

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

- The extension does not declare support for untrusted workspaces, so VS Code
  does not run it in Restricted Mode.
- Start commands and TypeScript previews run project code. Each checks trust
  again before running; untrusted, start commands are skipped with a log entry
  and discovered previews are replaced by the problem
  `Workbench previews require a trusted workspace.` Discovery may list
  definition files without trust, but never loads or compiles them.

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

The extension contributes no settings.

**Open Canvas in Browser** and `node server.js <project>` serve the same
canvas but differ in host. The browser command uses the extension's server, so
handoffs copy to the VS Code clipboard, source actions open files in the
editor, start commands and trust checks have already applied, and diagnostics
go to the output channel. The standalone server has no handoff (the handoff
route answers `available: false`), no source opening, no start commands, no
trust gate, and no log sink. Both show the same problems list as the sidebar
([core](core.md#problem-reporting)).

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
- The workbench tab is a webview containing the served workbench in an
  iframe, addressed through `asExternalUri`. The panel relays sidebar picks as
  `wb-go` and settled canvas selections as `wb-here`. It holds a pick until
  the iframe has reported `wb-here` and until any config refresh is
  acknowledged. The sidebar follows navigation initiated in the canvas,
  including links followed in a live page. Chats in the editor don't see
  the tab as context; [agent context](agent-context.md) covers how they learn
  which page it shows.
- Source actions ask the extension to open a file in the editor or reveal a
  directory; the server only opens paths the resolved config names. A handoff
  asks it to copy the generated prompt to the clipboard; the server has
  already saved the screenshot.

## Spaces

A future [multiple-artboard canvas](multiple-artboards.md#spaces-and-host-transport)
retains views from multiple servers while its host address stays fixed.
Its space picker browses without closing instances. The extension's space
switcher below still changes the editor's host space. Sidebar picks edit the
selected instance; canvas context and handoffs cover every instance.

Implemented in [spaces.js](../spaces.js), [extension.js](../extension.js),
and [space-switcher.js](../workbench/space-switcher.js). User guide:
[Several spaces](../docs/spaces.md).

- Spaces come from `workbench.yaml` files. A file without `spaces` is one
  space served from its folder; a file with `spaces` is one space per
  valid entry, each served from its `root` (relative to the file's folder,
  default that folder, so several may share one). Shared top-level keys are
  inherited, `implementations` merged by name; `name`, `color`, `icon`, and
  `root` are not (`selectSpace` in [manifest.js](../workbench/manifest.js),
  used by both readers). Folders are the window's that have a file, in folder
  order, then the folders the user added (stored in `globalState`, shared by
  every window). A folder in both is listed once, as the window's, and is not
  removable. An added folder whose `workbench.yaml` is missing is hidden but
  stays stored. Files are reread on every listing, so spaces added,
  removed, or re-rooted in the file show up without a reload; a running
  server whose space left the file, or whose root changed, stops.
- A space's id is the first 10 hex digits of the SHA-1 of its file's
  resolved folder, with `#<key>` appended for a space under `spaces` (so
  single-space ids are unchanged). Its name is the manifest `name`, else
  the folder name (single space) or a label from its key, and the folder
  name when the manifest does not parse.
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
  lists. Its mark takes the manifest's `color` (named
  or hex) and `icon` (Lucide name or project image), validated by
  `spaceMark` in [manifest.js](../workbench/manifest.js) for both readers,
  with `workbench.local.yaml` merged over them. Without a color, the color is
  derived from the id, except behind an image, which gets none. Images travel
  as data URIs of at most 256 KB, because every surface draws every
  space's mark and none can read another space's folder; a missing, large,
  or escaping image falls back to the initial, and a missing one is a config
  problem.
- The sidebar rereads the list on every build, and the canvas on every config
  load or refresh, so an edited name, color, or icon shows without a reload.
- One space is current per window. The window remembers it in
  `workspaceState` by path; otherwise, and when the remembered one leaves the
  list, the first listed space is current.
- One server per space, started on first open and kept until the space
  leaves the list or the extension stops. A window's own current space
  starts at activation (the single-space behavior); an added space starts
  when the view or canvas opens. A failed start is forgotten so the next open
  retries. All servers share one capture service, closed after them.
- Switching retargets the tab to the new server through the loading state and
  rebuilds the sidebar against the new root, watcher included. The previous
  space's selection is dropped. Switching is possible from the sidebar's
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
- `node server.js a b …` starts every space eagerly, shares one capture
  service, and prints one `workbench on <url>  <name>` line per space. One
  folder behaves and prints as before.

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

- The extension watches `workbench.yaml`, `workbench.local.yaml`, and every
  file matching the resolved preview discovery patterns (`previews.include`,
  or the defaults) for changes, creation, and deletion. Adding, removing, or
  renaming a definition, or changing its title, states, or viewports, updates
  the sidebar and canvas without **Refresh Pages**. With `previews: false`,
  only the YAML files are watched. **Refresh Pages** runs the same rebuild
  by hand.
- A rebuild first asks an open canvas to reload its config, then rebuilds the
  sidebar, which asks the server again and so re-runs preview discovery and
  catalog imports. Picks from the new sidebar wait until the canvas has
  acknowledged the refresh, so a pick of a new page or state does not target
  an old manifest. A refresh rejected for an invalid manifest keeps the
  queued pick until a later refresh succeeds.
- Edits to preview source other than the definition's page metadata are
  picked up by the worker on the next request; see
  [TypeScript previews](previews.md).

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

```text
VS Code extension host
  ├─ loopback workbench server ── project, workbench, and preview pages
  │    ├─ preview worker ── compiles and serves TypeScript previews (bundled runtime or Node)
  │    ├─ screenshot capture service ── bundled Electron helper
  │    └─ window capture helper ── ScreenCaptureKit stream of a Simulator or app window, macOS
  └─ VS Code webviews ── sidebar and workbench tab
```

The screenshot helper renders pages for [interactive capture](capture.md) and
[export](export.md). The window helper streams native windows for the
[Simulator and window lenses](implementations.md#native-window-stream); it is a separate process and
Screen Recording permission path. The preview worker and screenshot helper
share the bundled Electron runtime, unpacked once into the extension's global
storage. Browser-only VS Code cannot spawn any of these local processes.

### Preview worker

The worker runs project code, so it is a separate process from the extension
host and server. Its lifecycle, timeouts, and logging are specified in
[TypeScript previews](previews.md#compile-worker-lifecycle-and-trust).

## Logging

Server, canvas, capture, preview worker, and handoff diagnostics go to the
**Workbench** output channel as bounded event records. Each server stops
logging after 2 MB per session. Records never contain screenshots, page HTML,
or handoff prompts.

## Decisions

- **One server per space (2026-10-03).** The server, config readers,
  preview worker, agent view, and every route assume one root. A server per
  space keeps the single-space path unchanged and isolates each space's
  origin (and so its canvas storage); the cost is a port and a preview worker
  per opened space, so servers start lazily and share one capture service.
- **Added spaces are global, current space is per window (2026-10-03).**
  A user adds a space to Workbench, not to one window; which one a window
  shows is that window's state.
- **Watch preview definitions (2026-10-03).** Adding a preview should not need
  a manual refresh, so definition files are watched alongside the YAML files.

## Implementation gaps

- **Preview definitions are not watched.** The extension watches only the two
  YAML files ([sidebar-view.js](../sidebar-view.js)). Adding, removing, or renaming a
  definition takes effect only after **Refresh Pages** or a YAML change, and
  the user guide documents that limitation; update the guide when this is
  fixed.
- **Copy Canvas URL in remote workspaces (unverified).** The command copies
  the server's `127.0.0.1` address without `asExternalUri`; in a remote
  workspace that names the remote machine. The tab uses `asExternalUri`, and
  **Open Canvas in Browser** relies on `openExternal`.

- **Added spaces and workspace trust.** VS Code trust covers the open
  workspace only. An added space outside it runs start commands and
  previews under the window's trust flag; the guide tells users to add only
  folders they trust.

## Open questions

- **What is the trust gate for?** The extension declares no untrusted-workspace
  support, so VS Code never runs it in Restricted Mode, and the standalone
  server treats a missing trust flag as trusted. As built, no user can reach
  the start-command or preview trust checks. `isTrusted` is also read once at
  activation and passed to the server as a constant, so granting trust later
  would not be observed. Either declare limited untrusted support and make
  the gate live (reading trust changes), or remove the checks.

## Verification points

- [panel.test.js](../panel.test.js) checks the immediate loading tab, a closed
  loading tab staying closed, failed-startup recovery text, readiness when
  `wb-here` precedes iframe load, pick resends until acknowledged, and refresh
  ordering, including a refresh rejected for an invalid manifest.
- [sidebar-view.test.js](../sidebar-view.test.js) checks that catalog problems and
  catalog request failures reach the sidebar, and that showing the view opens
  the canvas.
- [sidebar.test.js](../workbench/sidebar.test.js) checks the waiting text,
  disabled search, the problems list, preview-only catalog requests, and the
  immediate build when previews are off.
- [spaces.test.js](../spaces.test.js) checks naming and fallback, ids,
  deduplication, lazy and single starts, retry after a failed start, and
  stopping on removal and close. [server.test.js](../server.test.js) checks
  the `/_workbench/spaces` routes, on-demand start through another server,
  and the cross-origin refusal. [panel.test.js](../panel.test.js) checks
  retargeting an open tab, a slower earlier load not winning, a closed tab
  staying closed, and `wb-space` relay. [sidebar-view.test.js](../sidebar-view.test.js)
  and [sidebar.test.js](../workbench/sidebar.test.js) check the
  `canonic-spaces` meta, watcher and resource retargeting, and the
  switcher's messages.
- [startup.test.js](../startup.test.js) checks probes, terminal startup only
  when the check fails, and skipping in untrusted workspaces.
- [server.test.js](../server.test.js) checks that only config-resolved paths
  open and that handoff is advertised only when the editor provides it.
- [preview.test.js](../preview.test.js) checks that the server starts, serves
  through, and closes the worker, stops serving after `previews: false`, and
  never runs definitions when untrusted or disabled.
- Manual, not automated: worker restart after it is killed, worker restart on
  a `previews` change, the YAML watcher, **Refresh Pages** after adding a
  definition (until definition watching is implemented and tested), activity-bar visibility without `workbench.yaml`, handoff
  from **Open Canvas in Browser**, and in VS Code: adding, switching, and
  removing spaces, the window-focus reread, and extension.js wiring. The
  canvas and sidebar switchers were exercised in headless Chrome against
  `node server.js` with two fixture spaces (2026-10-03).
