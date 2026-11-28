# Distribution

Signed binaries are planned for macOS, Windows and Linux on ARM64 and x64
(AMD64). The [install preview](https://canonic.sh/testrun/#install) shows the
one-line commands for each platform. Installation is pending.

If you already have a Testrun build, run `testrun --help` to verify command
discovery. Use `testrun --version` to identify the installed build and
`testrun docs list` to find the guides embedded in it.

## Host requirements

Binary availability and target support depend on different prerequisites. Check
the host for the target you plan to test before installing SDKs or browsers.
Use the [host and target chooser](getting-started.md#choose-your-host-and-target)
to find your first workflow.

| Target | Host requirements | Setup guide |
| --- | --- | --- |
| iOS simulators and physical iPhones | macOS with Xcode and the selected simulator runtime or device support | [iOS QA](ios.md#setup-and-readiness) |
| Android devices and emulators | Android SDK platform tools (`adb`). Emulator launches also need an installed emulator and AVD. | [Android QA](android.md#setup-and-readiness) |
| Isolated Chrome, Firefox and WebKit | An x64 Testrun build on macOS 15 or Windows. Headed Windows launches require the manager in the active console session. | [Browsers](browser.md) |

`testrun browser doctor --engine chrome` checks the browser host and prepared
dependencies without opening a browser or downloading anything. For mobile
targets, [check readiness](troubleshooting.md#the-device-is-missing-or-unavailable)
using the target's discovered ID.

## Update an existing installation

The binary includes generated Playwright and native CDP protocol schema data.
Their upstream notices are installed as `browser/PLAYWRIGHT-LICENSE`,
`browser/PLAYWRIGHT-NOTICE` and `browser/CDP-LICENSE` under
`PREFIX/share/testrun`. This does not install
Node, Playwright runtime files or browser builds. An explicit browser connection
prepares those private dependencies on the supported host; see [browser setup](browser.md).

Testrun starts without a configuration file. Agents and tools on the same host
use the one installed command; restart running Testrun processes, such as an
agent's MCP server, to load a new installation. The background device manager
keeps running the build that started it. When a new build uses another manager
protocol, the old manager retires once it is idle, or releases its devices and
retires when it is busy; meanwhile requests return retryable
`OWNER_UPGRADE_PENDING`. Otherwise run `testrun manager stop` (when no device or
client needs it) or `testrun manager drain`, and the next command starts the new build's
manager.
If the running manager lacks a capability required by a new command, use the
same stop or drain procedure before retrying that command.

## Diagnostic tools

Diagnostics bundle one component: the analysis parser, which is the `testrun`
executable itself and updates when you install a new build. Every other tool a
diagnostic source uses is yours to install and update; Testrun never downloads
or updates it. Install only the tools for the sources you use. A missing tool
affects only that source's sections; other commands and sources keep working.

| Source | Install on the host | Override |
| --- | --- | --- |
| Android logs, state and native capture | Android SDK platform tools (`adb`), on PATH | `TESTRUN_ADB` |
| iOS simulator and device | Xcode with its command-line tools (macOS only) | `TESTRUN_XCRUN` |
| iOS device logs | libimobiledevice's `idevicesyslog`, on PATH | `TESTRUN_IDEVICESYSLOG` |
| Apple symbolication | Xcode (`dwarfdump`, `atos`) | `TESTRUN_XCRUN` |
| Native traces on Apple targets | Xcode Instruments (`xctrace`) or macOS `sample` | `toolPath` |
| Perfetto or simpleperf conversion | Perfetto `trace_processor` or the NDK's simpleperf scripts | `toolPath` (required) |
| Box logs | The Box CLI, on PATH | none |

Apple sources need macOS. On another host they return `APPLE_HOST_UNSUPPORTED`.
React Native, Flutter and Dart sources connect to the app's own endpoint and
need no SDK on the host. See [diagnostic access](diagnostic-access.md) for
`diagnostics_capabilities` and `diagnostics_prepare`.
