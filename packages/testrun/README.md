# Testrun

Inspect and control apps from a CLI or coding agent.

Testrun provides one CLI and stdio MCP server for iOS simulators and physical
iPhones, Android devices and emulators, and browser sessions. Use it to read a
screen, send input, capture evidence and investigate failures. Firefox/WebKit
automation and Chrome application control have different capabilities; the
[browser guide](docs/browser.md) describes each engine's supported scope.
Use [pagination](docs/pagination.md) to read complete lists, UI trees and logs.

## Install

For development, build Testrun locally from source. You need Rust and the
prerequisites for your selected platform. See the
[host requirements](docs/distribution.md#host-requirements) for iOS, Android,
and browser testing, and [Distribution](docs/distribution.md) for public
installer availability.

From the Canonic repository root:

```sh
cd packages/testrun
cargo xtask install
```

This installs `testrun` into `~/.local/bin` and runner resources into
`~/.local/share/testrun`. Put `~/.local/bin` on PATH, or select another root with
`--prefix /path/to/prefix`. See [Distribution](docs/distribution.md) for host
requirements and manager replacement.
Run `testrun --help` to inspect the installed commands.

## Inspect a target

No configuration file is needed for ordinary QA. First discover available targets:

```sh
testrun target list --compact
```

This lists target IDs without acquiring control. After following the iOS
prerequisites and booting a simulator, replace `simulator:SELECTED-UUID` with
its listed ID:

```sh
testrun connect --target simulator:SELECTED-UUID
testrun observe --target simulator:SELECTED-UUID
testrun status --target simulator:SELECTED-UUID
testrun disconnect --target simulator:SELECTED-UUID
```

`observe` returns screen evidence, `status` reports the connection and work,
and `disconnect` releases the target. Android and browser setup have their
own [platform guides](#guides). The [Getting started guide](docs/getting-started.md)
also shows how to run a first browser check.

Physical iOS defaults to HID input and continuous video. HID sends gestures and
US ASCII keystrokes to current focus; it does not verify the focused field.
Choose the XCTest control path for accessibility inspection, guarded app input
and full-resolution screenshots. See [iOS setup](docs/ios.md) before controlling
a phone.

## MCP tools

Register Testrun in your project:

```sh
testrun mcp setup
```

Choose agents and review the changes, then complete the host's trust/reload
steps. To inspect an explicit selection without writes or prompts, use
`testrun mcp setup --agent codex --dry-run --json`. The
[MCP setup guide](docs/mcp.md#agent-setup) covers scopes, tool approvals,
repair and removal.

Your agent can use `target_list`, `connect`, `observe`, `act`, `status` and
`disconnect`, with additional tools for builds, captures and diagnostics.
[Tool catalog](docs/mcp.md#tool-catalog) lists the groups;
`testrun mcp serve --check` prints their schemas without opening a target.
The server runs through `testrun mcp serve`.

## CLI

Commands print readable results by default. Add `--json` for complete
machine-readable results; JSON mode does not prompt or emit human progress.
See [CLI output](docs/cli.md) for syntax, exit status, cancellation and recovery.

One background manager per user holds connected targets. CLI commands and MCP
servers share them. Copy the complete target ID from discovery or connect and
pass `--target` to address that connection explicitly. Closing a client leaves
targets available to other clients. Use `disconnect` to release a target and
`testrun manager` for inspection or recovery. Uncertain input is never repeated
automatically.

Optional `testrun.yml` and `testrun.local.yml` (or the same files in `.config/`,
or a file passed with `--config`) describe test builds and project workflows. Run `testrun init --dry-run --json` to inspect a detected project
configuration before creating it with `testrun init`.

## Guides

Read installed guides offline from any directory with `testrun docs list`,
`testrun docs search "connect"` or `testrun docs read cli.md`.

| Task | Start here |
| --- | --- |
| Capture a first mobile screenshot or start a browser | [Getting started](docs/getting-started.md) |
| Prepare and control mobile targets | [iOS](docs/ios.md), [Android](docs/android.md) |
| Automate pages and browser sessions | [Browser](docs/browser.md) |
| Build, install and launch test apps | [Test builds](docs/builds.md), [Projects](docs/projects.md), [Configuration](docs/configuration.md) |
| Capture video or hand input to a viewer | [Viewer control](docs/viewer-control.md), [Playback](docs/playback.md) |
| Investigate failures and inspect private evidence | [Investigations](docs/debugging.md), [Formats](docs/diagnostic-formats.md), [Access and export](docs/diagnostic-access.md) |
| Collect platform diagnostics | [Android logs](docs/diagnostic-android.md), [Android resource state](docs/diagnostic-android-state.md), [Apple](docs/diagnostic-apple.md), [Browser](docs/diagnostic-browser.md) |
| Inspect framework behavior | [React Native and Expo](docs/diagnostic-react-native.md), [Flutter and Dart](docs/diagnostic-dart.md) |
| Profile native work and memory ownership | [Native traces](docs/diagnostic-native-traces.md), [Memory and retention](docs/diagnostic-memory.md) |
| Investigate startup, rendering and UI costs | [Performance observations](docs/diagnostic-performance.md) |
| Follow requests and inspect data | [Network and data evidence](docs/diagnostic-network-data.md) |
| Investigate background, media and device context | [Scenario context](docs/diagnostic-scenario-context.md) |
| Inspect paused JavaScript and runtime checks | [Interactive debugging and diagnostic builds](docs/diagnostic-interactive.md) |
| Plan reproductions and compare attempts | [Reproduction and controlled comparisons](docs/diagnostic-reproduction.md) |
| Inspect runner failures and assert evidence | [Test results and assertions](docs/diagnostic-testing.md) |
| Inspect embedded pages and framework exports | [Framework and engine evidence](docs/diagnostic-frameworks.md) |
| Inspect desktop processes and service profiles | [Processes and service diagnostics](docs/diagnostic-services.md) |
| Diagnose setup, target or cleanup problems | [Troubleshooting](docs/troubleshooting.md) |
| Investigate deployed incidents and remote telemetry | [Remote observability](docs/diagnostic-observability.md) |
| Choose a debugging workflow and save a report | [Symptom workflows](docs/diagnostic-workflows.md), [Investigation reports](docs/diagnostic-reports.md) |
| Verify diagnostics and understand acceptance gaps | [Verification and Acme fixtures](docs/diagnostic-verification.md) |

## Development

[Development](docs/development.md) covers individual checks, fixtures and explicit
hardware/browser acceptance commands.

## License

Testrun is subject to the [Canonic Proprietary License](LICENSE).
Third-party components retain their own licenses and notices.
