# Testrun documentation

Testrun lets you inspect and control iOS, Android and browser targets from a CLI
or a coding agent. Use it to reproduce behavior, capture evidence, and investigate
app failures.

## Get started

- [Getting started](getting-started.md) helps you choose a host and target, then
  capture a mobile screenshot or check a browser's pages.
- [Distribution](distribution.md) covers installation availability, dependencies,
  and updating an existing installation.

## Test apps and browsers

<a id="test-an-app"></a>

- [iOS QA](ios.md) covers simulators, physical iPhones, and XCTest setup.
- [Android QA](android.md) covers devices, emulators, and screen and app operations.
- [Browsers](browser.md) covers isolated Chrome, Firefox and WebKit sessions.
- [Build, install and launch an app](builds.md) explains how to run a configured build.
- [Prepare an Expo project for device testing](projects.md) covers simulator and device preparation.
- [Playback state, controls and volume](playback.md) covers inspecting and controlling playback.
- [Give a local viewer input control](viewer-control.md) describes handing control to a viewer.

## Use the CLI and MCP

- [CLI output](cli.md) covers readable and JSON results, exit status, and recovery.
- [MCP transport](mcp.md) covers agent setup and the tool catalog.
- [Configure several projects](configuration.md) shows how to select a project and profile.
- [Read every page](pagination.md) shows how to follow cursors in paginated results.

## Investigate failures

- [Collect and inspect diagnostic evidence](debugging.md) introduces investigations and retained evidence.
- [Diagnostic access, privacy and preparation](diagnostic-access.md) explains prerequisites and collection scope.
- [Choose an investigation by symptom](diagnostic-workflows.md) helps pick the evidence to collect.
- [Reproduction and controlled comparisons](diagnostic-reproduction.md) covers comparable runs and controls.
- [Compose evidence and save an investigation report](diagnostic-reports.md) shows how to export findings.
- [Test results and evidence assertions](diagnostic-testing.md) covers checking outcomes against collected evidence.
- [Interactive debugging and instrumented builds](diagnostic-interactive.md) covers examining a running debug target.

## Platform and framework diagnostics

- [Investigate Android crashes, ANRs and disappearance](diagnostic-android.md) covers Android failures and logs.
- [Inspect Android state and resource series](diagnostic-android-state.md) covers scoped snapshots and trends.
- [Investigate iOS and iPadOS failures](diagnostic-apple.md) covers Apple logs, reports, and symbols.
- [Browser evidence in investigations](diagnostic-browser.md) covers retaining and querying browser results.
- [Frameworks, embedded pages and game evidence](diagnostic-frameworks.md) helps choose a source for your runtime.
- [React Native and Expo investigations](diagnostic-react-native.md) covers Hermes, React commits, and app markers.
- [Flutter and Dart investigations](diagnostic-dart.md) covers VM service capture and runtime evidence.
- [Desktop processes and service diagnostics](diagnostic-services.md) covers selecting process and service evidence.
- [Remote observability and deployed incidents](diagnostic-observability.md) covers correlating deployed evidence with an investigation.

## Reference

- [Diagnostic formats and query semantics](diagnostic-formats.md) covers accepted artifacts and query fields.
- [Startup, rendering and UI performance](diagnostic-performance.md) explains performance signals and their limits.
- [Native CPU, scheduling and hang investigations](diagnostic-native-traces.md) covers interpreting native traces.
- [Memory, allocation and retention investigations](diagnostic-memory.md) covers memory evidence and comparisons.
- [Network requests and data inspection](diagnostic-network-data.md) covers retained request evidence and data access.
- [Background, media, energy and device context](diagnostic-scenario-context.md) covers recording the conditions of a run.
- [Troubleshooting](troubleshooting.md) helps diagnose installation, target readiness, and cleanup problems.
