# Testrun changelog

## 0.20.0

### Mobile QA

- Discover iOS simulators, physical iPhones, Android emulators, and Android devices. Connect to an explicit target, inspect its screen and UI, send input, and capture screenshots or video.
- On physical iPhones, choose HID for full-display gestures and continuous video or the XCTest runner for accessibility inspection and app-scoped input. iOS testing requires macOS.
- Describe test builds, apps, and targets in `testrun.yml`. Prepare, build, install, launch, and inspect test apps through the CLI or MCP.

### Browser QA

- Open isolated Chrome, Firefox, and WebKit sessions on supported hosts. Navigate pages, inspect elements, send input, and capture screenshots. The [browser guide](https://canonic.sh/testrun/docs/browser/) describes engine and host differences.
- Exercise page workflows with explicit frame and page selection, conditional waits, file transfer, and reusable app sessions. Capture evidence through screenshots, recordings, and viewer control on supported engines.

### Diagnostics and agents

- Collect logs, traces, test failures, and network evidence into investigations. Compare attempts and export the evidence needed for a report.
- Register Testrun with a coding agent using `testrun mcp setup`. CLI and MCP clients share the same background manager, so a selected test connection can survive a client exit.
- Read the guides offline with `testrun docs list`, `testrun docs search`, and `testrun docs read`.

### Downloads

- Portable x64 and ARM64 archives for Linux, macOS, and Windows. Linux archives include Canonic signatures. Windows and macOS downloads are offered without publisher signing. For platform requirements, see the [distribution guide](https://canonic.sh/testrun/docs/distribution/).
