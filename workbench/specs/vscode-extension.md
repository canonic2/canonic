# VS Code extension contract

This spec covers the editor host around the [core workbench](core.md). Its
implementation is in [extension.js](../extension.js), [panel.js](../panel.js),
[screens.js](../screens.js), [startup.js](../startup.js),
[server.js](../server.js), and [preview-service.js](../preview-service.js).
The user-facing reference is [The VS Code extension](../docs/extension.md).

## Activation and ownership

- The extension activates when the workspace contains `workbench.yaml`. A
  command can also activate it elsewhere, but without that file it serves no
  workbench and explains why the command cannot open one.
- In a multi-root window, the first folder with `workbench.yaml` owns the
  window's workbench. The extension starts one loopback server for that folder,
  contributes the Workbench sidebar view, and closes the server and its helper
  processes on disposal. The Workbench activity-bar view is hidden without a
  workbench.
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
| **Workbench: Refresh Screens** | Refreshes the canvas and rebuilds the sidebar (see [Refresh](#refresh)). |
| **Workbench: Show Log** | Shows the **Workbench** output channel. |

`canonic.capture.chromePath` is the only setting: a browser executable for
fallback screenshots ([capture](capture.md)).

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
  brings it forward, and leaves keyboard focus in the list. Closing the tab
  while the view stays visible leaves it closed until the view is selected
  again.
- The tab opens before implementation readiness and remote port forwarding
  finish, showing a themed **Opening Workbench** state. Server failure replaces
  it with recovery instructions that point to the log. Delayed startup must
  not reopen a tab the user closed, and the latest screen pick wins during
  startup.
- The workbench tab is a webview containing the served workbench in an
  iframe, addressed through `asExternalUri`. The panel relays sidebar picks as
  `wb-go` and settled canvas selections as `wb-here`. It holds a pick until
  the frame has reported `wb-here` and until any config refresh is
  acknowledged. The sidebar follows navigation initiated in the canvas,
  including links followed in a live page.
- Source actions ask the extension to open a file in the editor or reveal a
  directory; the server only opens paths the resolved config names. A handoff
  asks it to copy the generated prompt to the clipboard; the server has
  already saved the screenshot.

## Sidebar and problems

- The Workbench sidebar view is an extension webview that reads the YAML
  itself and uses the same navigation model as the canvas.
- When previews are enabled or any implementation has a catalog, it asks the
  extension for the server's imported sections and `problems` before building.
  Until they arrive it shows **Waiting for** the pending catalogs, or
  **Finding previews…**, and disables search.
- Problems from that answer are listed above the screens. If the request
  itself fails, that failure is listed instead and authored screens still
  build.
- A YAML file the sidebar cannot read replaces the list with its error.

## Refresh

- The extension watches `workbench.yaml`, `workbench.local.yaml`, and every
  file matching the resolved preview discovery patterns (`previews.include`,
  or the defaults) for changes, creation, and deletion. Adding, removing, or
  renaming a definition, or changing its title, states, or viewports, updates
  the sidebar and canvas without **Refresh Screens**. With `previews: false`,
  only the YAML files are watched. **Refresh Screens** runs the same rebuild
  by hand.
- A rebuild first asks an open canvas to reload its config, then rebuilds the
  sidebar, which asks the server again and so re-runs preview discovery and
  catalog imports. Picks from the new sidebar wait until the canvas has
  acknowledged the refresh, so a new row does not target an old manifest. A
  refresh rejected for an invalid manifest keeps the queued pick until a later
  refresh succeeds.
- Edits to preview source other than the definition's screen metadata are
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
  │    ├─ screenshot capture service ── bundled Electron helper or Chrome fallback
  │    └─ window capture helper ── ScreenCaptureKit stream of a Simulator or app window, macOS
  └─ VS Code webviews ── sidebar and workbench tab
```

The screenshot helper renders pages for [interactive capture](capture.md) and
[export](export.md). The window helper streams native windows for the
[Simulator and window lenses](other-previews.md); it is a separate process and
Screen Recording permission path. The preview worker and screenshot helper
share the bundled Electron runtime, unpacked once into the extension's global
storage. Browser-only VS Code cannot spawn any of these local processes.

### Preview worker

The worker runs project code, so it is a separate process from the extension
host and server. What it compiles and serves is specified in
[TypeScript previews](previews.md); this section covers its lifecycle.

- It is started lazily by the first config resolution that discovers at least
  one definition in a trusted workspace with previews enabled. Packaged builds
  run it on the bundled runtime as Node; otherwise it uses the server's own
  Node. It must report its port within 30 seconds or startup fails with a
  problem.
- A change to the resolved `previews` value restarts it on the next
  resolution. `previews: false` stops it.
- If it exits unexpectedly, the next preview request or config resolution
  starts a new one. There is no proactive restart.
- It stops with the server: the server asks it to close and kills it after
  3 seconds. It also exits when its parent's IPC channel disconnects.
- Its error output is logged as `preview.worker` (each chunk truncated), and
  per-request timing as `preview.request.completed`.

## Logging

Server, canvas, capture, preview worker, and handoff diagnostics go to the
**Workbench** output channel as bounded event records. Each server stops
logging after 2 MB per session. Records never contain screenshots, page HTML,
or handoff prompts.

## Decisions

- **Watch preview definitions (2026-10-03).** Adding a preview should not need
  a manual refresh, so definition files are watched alongside the YAML files.

## Implementation gaps

- **Preview definitions are not watched.** The extension watches only the two
  YAML files ([screens.js](../screens.js)). Adding, removing, or renaming a
  definition takes effect only after **Refresh Screens** or a YAML change, and
  the user guide documents that limitation; update the guide when this is
  fixed.
- **Possible duplicate preview workers (hypothesis, from code reading).**
  `importPreviews` in [server.js](../server.js) closes the old worker on a
  `previews` change, then sets `previews = null` after an `await`. Two
  overlapping resolutions after a YAML edit (the sidebar's catalog request and
  the canvas refresh both resolve the config) could each pass that check; the
  later one would then discard the worker the earlier one created without
  closing it. That worker would run until the extension host exits. Not
  reproduced.
- **Idle worker after previews disappear.** When discovery finds no
  definitions, resolution returns before touching the running worker, so it
  keeps running until the server stops or `previews` changes.
- **Copy Canvas URL in remote workspaces (unverified).** The command copies
  the server's `127.0.0.1` address without `asExternalUri`; in a remote
  workspace that names the remote machine. The tab uses `asExternalUri`, and
  **Open Canvas in Browser** relies on `openExternal`.
- **Trust read once.** `isTrusted` is read at activation and passed to the
  server as a constant. This is harmless while the extension does not run in
  Restricted Mode, but would be wrong if untrusted support were declared.

## Open questions

- Should an unexpected worker exit be logged and reported as a problem, rather
  than surfacing only as a failed request until the next one restarts it?

## Verification points

- [panel.test.js](../panel.test.js) checks the immediate loading tab, a closed
  loading tab staying closed, failed-startup recovery text, readiness when
  `wb-here` precedes iframe load, pick resends until acknowledged, and refresh
  ordering, including a refresh rejected for an invalid manifest.
- [screens.test.js](../screens.test.js) checks that catalog problems and
  catalog request failures reach the sidebar, and that showing the view opens
  the canvas.
- [sidebar.test.js](../workbench/sidebar.test.js) checks the waiting text,
  disabled search, the problems list, preview-only catalog requests, and the
  immediate build when previews are off.
- [startup.test.js](../startup.test.js) checks probes, terminal startup only
  when the check fails, and skipping in untrusted workspaces.
- [server.test.js](../server.test.js) checks that only config-resolved paths
  open and that handoff is advertised only when the editor provides it.
- [preview.test.js](../preview.test.js) checks that the server starts, serves
  through, and closes the worker, stops serving after `previews: false`, and
  never runs definitions when untrusted or disabled.
- Manual, not automated: worker restart after it is killed, worker restart on
  a `previews` change, the YAML watcher, **Refresh Screens** after adding a
  definition (until definition watching is implemented and tested), activity-bar visibility without `workbench.yaml`, and handoff
  from **Open Canvas in Browser**.
