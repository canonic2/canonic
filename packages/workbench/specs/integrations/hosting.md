# Hosts, capabilities and updates

This topic owns the capability differences between the editor, standalone
browser, portable output and future hosts. It collects the existing
[host guide](../../docs/extension.md#running-without-vs-code) rather than
introducing a new host or distribution channel.

## Host capability model

| Host | Services and limits |
| --- | --- |
| Desktop VS Code | Extension-owned space servers, editor/source actions, clipboard handoff and configured implementation start commands |
| Browser opened by the extension | Uses the extension's running server and host services; closing this browser does not close the editor's servers |
| Standalone Node host | Same native page/server/capture services, with no editor source actions, clipboard handoff or configured start-command runner; switch among supplied roots |
| Portable viewer | Static compiled output and declared data, including local state/input controls; no authored-file editing, live catalog discovery or host-owned capture service. Astro input changes requiring server rendering are unavailable |
| Browser-only VS Code | Cannot spawn the native server/helper; no local extension runtime contract |
| Studio | Future integration; no server startup/protocol or library embedding acceptance established |

Feature availability follows the actual host/service capability. Do not infer
editor authority merely from running in a browser, or show a successful source
open/handoff when no host can perform it. Rendering or compilation failure
remains a destination problem, not evidence that the host lacks the feature.
[Agent context](agent-context.md) and [export](../review/export.md) own their
specific interfaces.

## Platform and dependency boundaries

- The desktop extension executes as Node 24+ under the supported VS Code
  engine range declared in [extension/package.json](../../extension/package.json).
  The standalone Node launcher likewise requires Node 24+. The native binary
  must match the machine running the extension/standalone host.
- Native server binaries and the pinned Electron runtime ship in target-specific
  VSIX packages. Runtime preparation uses installed assets; it does not download
  a compiler/browser on first page selection. Project adapters still use the
  project's own packages, as their specs require.
- Native Simulator/window lenses need macOS and their host prerequisites.
  The screenshot runtime requires macOS 13+; Linux capture needs a display.
  Missing capture prerequisites give a named failure and do not imply the
  page-list/server code cannot run. The owning [capture contract](../review/capture.md#bundled-electron-runtime)
  specifies evaluator fallback and exact preparation behavior.
- A remote workspace runs services on the remote machine. Its platform,
  display, filesystem and loopback are the remote host's, and editor URL
  forwarding is distinct from copying a literal server address. Remote URL
  command gaps remain in [VS Code](vscode-extension.md#implementation-gaps).
- Packaging for six targets is not proof of execution on all six. Current
  evidence is strongest on desktop macOS; Linux/Windows and remote host
  acceptance remain limited. Do not infer browser compatibility from a
  Chromium/VS Code smoke. Portable output needs its own browser acceptance.

## Update, close and uninstall

Installing a replacement VSIX changes files on disk. Already running windows,
servers, evaluators and capture helpers keep their loaded version until the
editor window reloads. **Refresh Pages** or a canvas reload is not an extension
process upgrade. Normal shutdown stops each space's owned processes and then
the shared capture service; an independent standalone host remains its own owner.

Uninstall followed by window shutdown/reload stops the running extension.
Authored pages/configuration and saved handoff files remain user project data;
uninstall is not a project cleanup operation. Runtime cache cleanup and editor
storage retention belong to their owners. Do not promise that uninstall erases
every local preference or captured artifact.

## Acceptance and gaps

- Verify each command against the actual host: source opening, handoff,
  start commands, browser downloads and multi-root switching.
- A missing/wrong-target native server binary prevents that space from starting
  and gives a useful failure. An unavailable capture runtime fails capture;
  unrelated rendering/services remain usable where their own prerequisites exist.
- Install/update in an active window, then reload it; ensure replacement
  services run the new build and old owned processes stop. A closed tab stays
  closed under the [restoration contract](../canvas/persistence.md).
- Test portable output without the original project/dependencies/server, and
  keep live-only controls unavailable rather than issuing dead requests.

Packaging and process tests are linked by [modules](../system/modules.md#asset-and-packaging-checks)
and [server](../system/server.md#verification). The
[installed evidence](../research/README.md) records fixture/build/host limits.
No complete cross-platform/browser matrix, browser-only editor implementation
or Studio integration is established by those checks.
