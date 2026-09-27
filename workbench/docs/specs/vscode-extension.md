# VS Code extension contract

This spec covers the editor host around the [core workbench](core.md). Its
implementation is in [extension.js](../../extension.js),
[panel.js](../../panel.js), [screens.js](../../screens.js),
[startup.js](../../startup.js), and [server.js](../../server.js).

## Activation and ownership

- The extension activates when the workspace contains `workbench.yaml`. A
  command can also activate it elsewhere, but without that file it serves no
  workbench and explains why the command cannot open one.
- In a multi-root window, the first folder with `workbench.yaml` owns the
  window's workbench. The extension starts one loopback server for that folder,
  contributes a Canonic status item and Screens sidebar, and closes the server
  on disposal. The Canonic activity-bar view is hidden without a workbench.
- The server serves project files at `/` and packaged workbench files at
  `/_workbench/` on the same origin. Commands open its URL in a VS Code editor
  tab or an external browser, or copy the URL. Server and capture failures go
  to the **Canonic Workbench** output channel.
- A newly installed build needs a VS Code window reload before that window
  runs its new extension code.

## Editor surfaces and messages

- The Screens sidebar is an extension webview. It uses the same navigation
  model as the canvas and receives imported catalog data from the server
  through the extension host. Config watchers rebuild it after asking the
  canvas to refresh, so a new row does not target an old manifest.
- The workbench tab is another webview containing the served workbench in an
  iframe. The panel relays sidebar picks as `wb-go` and settled canvas
  selections as `wb-here`. It queues a pick while its frame or config refresh
  is pending. The sidebar follows navigation initiated in the canvas.
- Source actions ask the extension to open a file in the editor or reveal a
  directory. A handoff asks it to copy the generated prompt to the clipboard;
  the server has already saved the screenshot. A browser-only workbench has no
  editor source action or VS Code clipboard handoff.

## Implementation startup

- In a trusted workspace, each configured URL or Storybook implementation may
  define `start.command`, `start.cwd`, `start.check`, `start.ready`, and
  `start.timeout`. The extension checks the configured TCP port or HTTP URL
  first. If it is already available, it leaves the terminal alone. Otherwise
  it opens a VS Code terminal in the configured directory, runs the command,
  and waits for readiness before starting the workbench server.
- Startup failures are logged and do not prevent the server from starting;
  the affected preview or catalog can report its own problem. The standalone
  server and browser do not run configured startup commands.

## Process boundaries

```text
VS Code extension host
  ├─ loopback workbench server ── project and workbench pages
  ├─ VS Code webviews ── sidebar and workbench tab
  ├─ screenshot capture service ── bundled Electron helper or Chrome fallback
  └─ Simulator stream helper ── ScreenCaptureKit and WDA path on macOS
```

The screenshot helper renders web pages for [interactive capture](capture.md)
and [export](export.md). The Simulator helper streams a native device window;
it is a separate process and permission path. Browser-only VS Code cannot
spawn these local helpers.

## Verification points

- [panel.test.js](../../panel.test.js) checks tab reuse and message ordering.
- [sidebar.test.js](../../workbench/sidebar.test.js) checks sidebar refresh.
- [startup.test.js](../../startup.test.js) checks probes and terminal startup.
- [server.test.js](../../server.test.js) checks serving and editor callbacks.
