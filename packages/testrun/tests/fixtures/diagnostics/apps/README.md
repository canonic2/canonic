# Disposable Acme diagnostic apps

These apps belong to Testrun's fixtures. Acceptance can use them instead of a
user project. The native projects are checked in; framework projects are created
by the selected upstream tools, then receive the checked-in app entry points and
shared fault sources. No generated native tree or dependency cache is vendored.

| App | Project / entry point | Faults |
| --- | --- | --- |
| Native iOS | `ios/AcmeDiagnostics.xcodeproj` | Swift fatal/background error, Objective-C exception, C++ abort, six-second main-thread hang, Swift/native retained memory |
| Native Android | `android/` | Java/Kotlin caught and uncaught errors, background error, six-second main-thread hang, self-kill, retained memory |
| Native macOS | `macos/` | AppKit UI, Swift/Objective-C/C++ failures, bounded CPU, six-second main-thread hang, background error, retained memory |
| Native Windows | `desktop/windows.cpp` | Win32 UI, caught C++ exception, armed abort/background abort, six-second UI-thread hang, bounded CPU, retained memory |
| Native Linux | `desktop/linux.cpp` | GTK 3 UI with the same C++ controls; uses the selected X11/Wayland desktop |
| Expo | `expo/App.js` plus selected Expo blank template | JS exception, bounded CPU work, retained buffers, state churn, simulated network/bridge failures |
| Bare React Native | `react-native/App.tsx` plus selected Community CLI template | The same JS controls in a distinct native RN project |
| Flutter | `flutter/pubspec.yaml` plus `../../flutter/main.dart` | Dart exception, bounded CPU work, retained buffers, state churn, simulated network/plugin failures |
| Web | `web/` | Vite vanilla, React, Vue, Svelte, Solid, Preact, Lit, Angular JIT, Next.js, Nuxt and Astro; exceptions, bounded CPU/retention, scheduled state updates, simulated bridge/network failures |

Native mobile apps show a persisted launch counter and a fresh screen-birth UUID after
each creation. Android configuration changes can also recreate the screen; neither
value proves a new process. Pair them with the collector's actual process birth
and runtime generation. Native destructive buttons require the app's arm toggle;
it resets after each request. Corrected mode turns fatal/hang/kill triggers into
controls. Fatal branches are intentionally uncaught. Background failures can
terminate the process. Reset drops fixture memory; it does not certify GC/ARC.
The iOS app has no self-kill button; external lifecycle belongs to Testrun.

## Commands

Every command below runs from the Testrun package (`packages/testrun`). The
`tools/` scripts take positional arguments in the order shown and refuse a
missing or relative path; pass `""` for an optional argument you leave
unselected before one you select.

## Build native apps

Using installed Xcode:

```sh
xcodebuild -quiet -jobs 2 -project tests/fixtures/diagnostics/apps/ios/AcmeDiagnostics.xcodeproj -scheme AcmeDiagnostics -configuration Debug -sdk iphonesimulator -destination 'generic/platform=iOS Simulator' -derivedDataPath /private/tmp/acme-diagnostics-derived CODE_SIGNING_ALLOWED=NO build
```

The unsigned simulator package is
`/private/tmp/acme-diagnostics-derived/Build/Products/Debug-iphonesimulator/AcmeDiagnostics.app`.
`ios/testrun.yml` also supplies Xcode Debug/Release profiles for Testrun's existing
build workflow. Device signing remains a separate build choice.

### Explicit iOS simulator acceptance

`tools/accept-diagnostic-ios.mjs` takes a retained CLI artifact, this built
app, a new private evidence root, an installed runtime identifier and an
optional case selection. It creates an
iPhone 16 in its own custom device set. Testrun owns boot, app installation,
accessibility input, installed/process/log observations and investigation
analysis; no user simulator or physical device is selected. Use a short root:
managed mobile control endpoints must fit macOS's Unix socket path limit.

```sh
node tools/accept-diagnostic-ios.mjs /absolute/pinned/testrun /absolute/AcmeDiagnostics.app /private/tmp/acme-i1 com.apple.CoreSimulator.SimRuntime.iOS-26-2 all
```

The source uses the exact simulator UUID plus explicit device-set directory;
the UI actor uses the discovered custom-set target ID. The report retains both.
Switch identifiers and bounded native action/result logs let the run verify
actual controls, retained Swift/C++ buffers, reset and corrected fatal controls.
Observed UI counts feed the same defective/corrected assertion through Testrun.
Counts describe fixture ownership, not a heap census or RSS release.

Native stack capture selects only that simulator app's host PID, checks its
kernel birth/path continuity and retains one-second samples before/during an
injected main-thread sleep, followed by a responsive reset. This is host-process
sampling, not physical iOS or an Instruments-template certification. Relaunch
must produce a different observed native process generation.

Fatal cases use an explicit native `simctl launch --console-pty` with an open
input transport to retain the selected app's bounded console streams in the
private run root, then Testrun input triggers its armed
Swift/Objective-C/C++/background controls. Native breadcrumbs and process
disappearance are retained through diagnostics; neither infers an OS signal or
becomes an IPS report. Global crash-folder discovery, watchdog/Jetsam, physical
device reports, independent symbolication and causal overhead remain separate.
The run closes its investigations/manager and deletes only its new simulator;
private evidence and SDK/build caches remain. No computer-use tool is needed.

A final case argument of `ui`, `trace` or `faults` selects a smaller campaign;
the report preserves that selection. The default `all` includes every case.
`sample-probe` is one diagnostic-only native-tool run with a separate twenty-second
deadline; any successful import validates that artifact, not product capture.
`functional` also runs fatal/sleep recovery controls and this diagnostic probe,
while explicitly excluding a successful native capture claim. Console processes
are reaped only after their app ends or the owned simulator is shut down.

For Android, select installed Gradle 9.3.1, JDK 17+, Android SDK 36 and cached
AGP 9.1.0 dependencies. AGP's built-in Kotlin compiles the Kotlin fault source.

For reusable setup and build on macOS/Linux:

```sh
node tools/setup-diagnostic-android.mjs
```

The setup script downloads Gradle 9.3.1 from its official distribution service,
verifies its SHA-256 checksum before extraction and builds the APK, allowing
Gradle to acquire the fixture's build dependencies. On macOS the persistent cache
is `~/Library/Caches/testrun/development/android`; on Linux it is
`${XDG_CACHE_HOME:-~/.cache}/testrun/development/android`. Repeated calls reuse
that Gradle distribution and its dedicated dependency cache. SDK/JDK installation
and emulator startup are outside this script.

It uses `ANDROID_HOME`/`ANDROID_SDK_ROOT`, or the usual host SDK directory;
`JAVA_HOME`, or macOS's installed JDK 17/Linux's `/usr/bin/java`. Override them
with absolute cache, SDK and JDK arguments, in that order. A fourth argument of
`0` builds a warmed cache without network:

```sh
node tools/setup-diagnostic-android.mjs "" "" "" 0
```

Offline mode refuses missing cached Gradle inputs/dependencies. Cache contents
survive build failures; partial downloads/extractions owned by a failed setup
are removed. `node --test tools/setup-diagnostic-android.test.mjs` checks cold/warm/offline behavior
and integrity refusals using owned transport/extraction peers without downloads.

To use an already installed Gradle directly instead:

```sh
ANDROID_HOME=/absolute/android-sdk /absolute/gradle/bin/gradle -p tests/fixtures/diagnostics/apps/android --offline --no-daemon --console=plain assembleDebug
```

The build is offline and produces `android/app/build/outputs/apk/debug/app-debug.apk`.
It uses Android's local debug signing, never a publishing key. Missing caches
are a dependency prerequisite; drop `--offline` explicitly to download build
dependencies.
The APK is profileable by shell and Debug is debuggable. Neither native build
starts, discovers, installs on or changes a device.

### Explicit Android emulator acceptance

```sh
node tools/accept-diagnostic-android.mjs /absolute/pinned/testrun /absolute/app-debug.apk /absolute/android-sdk /private/tmp/acme-a1 /absolute/jdk
```

The arguments are the CLI, APK, SDK, new evidence root, JDK and an optional
simpleperf executable.

This contributor harness requires the installed API 36 Google APIs Play Store
x86_64 image and creates a uniquely named Pixel 6 AVD in a new private directory.
Testrun owns its headless read-only start, installation, accessibility input and
release. It verifies the actual AVD name before touching the app. No physical
device or user's existing AVD is selected; the shared ADB server is preserved.

The checks cover caught Java/Kotlin exceptions, logical retained bytes,
reset/corrected controls, the same failed/corrected assertion, installed/process/
log identity, resource snapshots, six-second sleep/recovery, relaunch generation,
uncaught Java/Kotlin/background faults and self-kill. Logs require the selected
PID's exact native trigger breadcrumb. Native exception output is retained and
queried through Testrun; disappearance alone does not infer a signal or cause.
Logical bytes and OS memory accounting do not establish heap ownership or GC.

Optionally add `/absolute/native/host/simpleperf` as a sixth argument for a
one-second 99 Hz recording attempt. The harness does not deploy a device binary,
enable root or change profileability. Its result stays explicitly unavailable
if the native capture cannot succeed; other functional cases cannot certify it.
ANR/watchdog, native tombstones/unwinding, physical/OEM/release behavior, capture
loss and causal overhead remain separate. Eleven functional cases passed on
API 36 x86_64/Debug. The same run found an unavailable
simpleperf attempt. Other native/build cells remain unverified.

The private report retains CLI/APK/harness digests, SDK image metadata, bounded
command results and collector digests. Cleanup closes investigations, retires
only its private manager, checks the owned emulator left ADB, and deletes only
the new AVD. Evidence and SDK/build caches remain.
The harness also verifies the running manager's executable digest against the
selected CLI. When build and execution contexts differ, verify the artifact in
the execution context; a matching output filename does not establish fresh bytes.

If a run reports unfinished cleanup, preserve its report and retry only that
run's release through the same acceptance script's `--cleanup` option. It
requires no APK or new CLI selection and starts no campaign:

```sh
node tools/accept-diagnostic-android.mjs --cleanup /private/tmp/acme-a1 /absolute/android-sdk /absolute/jdk
```

Cleanup mode shares the retained-run cleanup implementation. Normal acceptance attempts cleanup in its
finalizer on both success and failure; cleanup mode retries an interrupted or
failed release without repeating app input.

It verifies the retained CLI digest and fixture ownership, releases the original
AVD alias and any retained serial entry, requires manager retirement and observed
serial absence, then deletes only that run's named AVD. The initial failure stays
in the acceptance report alongside the cleanup recovery result.

## Build desktop apps

The macOS fixture is an independent AppKit `.app`, not an embedded browser shell:

```sh
mkdir -p /private/tmp/acme-macos-diagnostics/AcmeDiagnostics.app/Contents/MacOS
xcrun clang -g -O2 -fobjc-arc -c tests/fixtures/diagnostics/apple/AcmeObjCFaults.m -o /private/tmp/acme-macos-diagnostics/AcmeObjCFaults.o
xcrun clang++ -g -O2 -std=c++17 -c tests/fixtures/diagnostics/native/acme_faults.cpp -o /private/tmp/acme-macos-diagnostics/acme_faults.o
xcrun swiftc -g -O -parse-as-library -import-objc-header tests/fixtures/diagnostics/apps/ios/AcmeBridge.h tests/fixtures/diagnostics/apps/macos/AcmeApp.swift tests/fixtures/diagnostics/apple/AcmeSwiftFaults.swift /private/tmp/acme-macos-diagnostics/AcmeObjCFaults.o /private/tmp/acme-macos-diagnostics/acme_faults.o -framework AppKit -lc++ -o /private/tmp/acme-macos-diagnostics/AcmeDiagnostics.app/Contents/MacOS/AcmeDiagnostics
cp tests/fixtures/diagnostics/apps/macos/Info.plist /private/tmp/acme-macos-diagnostics/AcmeDiagnostics.app/Contents/Info.plist
```

It produces `/private/tmp/acme-macos-diagnostics/AcmeDiagnostics.app` for the
selected host compiler/SDK, optimized with debug symbols. Building does not launch,
sign or install it. Close
its last window to quit; ordinary quit/reset releases fixture memory. UI hang,
background crash and all fatal controls require arming. Its architecture and
minimum deployment are determined by the selected compiler, not a universal
macOS support declaration.

The explicit macOS acceptance run launches only this app with `--acme-driver`,
which accepts at most 64 fixed stdin commands of at most 64 bytes. It invokes the
same `NSButton.performClick` handlers and emits bounded app-owned observations;
it is not OS accessibility/input or screenshot automation. Ordinary launches have
no driver. Driver EOF quits the fixture. Corrected/fatal actions, Testrun process
snapshots and exact-PID stack sampling use a new private evidence root and manager.

```sh
cargo build --locked
node tools/accept-diagnostic-macos.mjs /absolute/testrun /absolute/AcmeDiagnostics.app /absolute/new-private-run all
```

The arguments are the CLI, app, new evidence root, a case selection and, for
the `ips` case, the report path and its PID. The run pins the selected CLI, refuses old capabilities, records app/CLI
digests, verifies byte ownership separately from RSS, imports the same defective/
corrected condition through Testrun, checks finite native capture, relaunch birth,
actual own-process fatal exits and cleanup. Fatal exit events are observations
of owned children; they do not become OS IPS reports. Automatic global report
discovery and other apps are outside the run. Evidence remains private;
investigations are removed and the run's manager and children stopped.

The case `faults` selects only corrected/fatal controls;
`objc` narrows that to the Objective-C trigger. The latter runs in an Objective-C
background callback, emits a fixed exception-request breadcrumb and preserves
the actual native signal. The recorded macOS host exits with SIGSEGV in native
exception unwinding; acceptance records that discrepancy instead of assuming
every uncaught exception is SIGABRT. A known native failure is useful evidence,
not proof of a healthy native unwinder.

For an existing owned report, select the case `ips` followed by the report's
exact absolute path and its observed numeric PID.
This separate case verifies the selected Acme PID and imports/parses its Apple
header/body report through Testrun. It preserves the original, discovers no files
and does not independently symbolize native frames. The maintained case checks
the recorded Objective-C SIGSEGV and exception/unwind frames, not every report.

The case `sample-probe` or `sample-probe-late-symbols` selects
one diagnostic-only native-tool probe with a 20-second bound and owned output,
respectively with eager or deferred symbol lookup. These bypass the collector
only to diagnose the tool, never to count as a product acceptance pass. The
ordinary capture retains its existing deadlines. Small shared-host action pairs
include AppKit button processing and do not establish causal capture overhead,
tail latency or a shipping budget. No computer-use permission is required.

Windows and Linux share a C++17 fault controller, compiled into actual native
Win32/GTK windows. On Windows use a Visual Studio developer terminal with the
Desktop C++ workload, Windows SDK and CMake. On Linux install CMake, a C++17
compiler, pthread support, pkg-config and GTK 3 development headers/libraries.
From the Testrun package (replace the output path for the selected host):

```sh
cmake -S tests/fixtures/diagnostics/apps/desktop -B /absolute/acme-desktop-build
cmake --build /absolute/acme-desktop-build --config Debug --parallel 2
ctest --test-dir /absolute/acme-desktop-build --build-config Debug --output-on-failure -R bounded-desktop-controls
```

The GUI executable is `AcmeDiagnostics` on Linux, or usually
`Debug/AcmeDiagnostics.exe` for Visual Studio's multi-configuration generator.
Run only that owned fixture in the selected desktop session. A background abort
affects its own process, not another service. These apps do not install agents,
services, system-wide crash handlers or external profilers. Ordinary close/reset
releases the held native buffer. An intentional abort relies on OS process exit
for reclamation and can generate an OS crash report/dump under the host policy.
No ANR/watchdog, dump creation or upstream symbolication is guaranteed.

The non-GUI C++ controls can be selected independently of GTK/Win32:

```sh
cmake -S tests/fixtures/diagnostics/apps/desktop -B /absolute/acme-desktop-controls -DACME_CONTROLS_ONLY=ON
cmake --build /absolute/acme-desktop-controls --target acme-controls --config Debug --parallel 2
ctest --test-dir /absolute/acme-desktop-controls --build-config Debug --output-on-failure -R bounded-desktop-controls
```

That target selects `ACME_CONTROLS_ONLY=ON`; it never executes defective abort,
background abort or hang paths. A fresh normal CMake build is required for a GUI.

## Prepare web apps

Web fixtures are checked-in app entry points, not generated examples from a
moving upstream template. [frameworks.json](web/frameworks.json) lists each
adapter's exact package names. Preparation copies them into a new owned project
and writes a private package manifest from selected exact dependency versions;
it runs no generator, dependency installer, network request or server.

Supply an absolute JSON file mapping each required package name to an exact
`major.minor.patch` version (prerelease suffixes are accepted). For example the
React adapter requires `vite`, `react` and `react-dom`; Vue requires `vite` and
`vue`. Use mutually compatible actual versions; no `latest`, ranges or implicit
framework/plugin upgrades are accepted. This input file can cover several apps.

```sh
node tools/prepare-diagnostic-web.mjs vue /private/tmp/acme-vue-diagnostics /absolute/web-versions.json
```

Registered framework names are `vanilla`, `react`, `vue`, `svelte`, `solid`,
`preact`, `lit`, `angular`, `next`, `nuxt`, and `astro`. Svelte uses the Svelte 5
mount API; Vue/Nuxt use Vue 3; React/Next use React 18+ create-root/hooks; Angular
uses zoneless signal change detection and browser JIT, not Angular CLI/AOT or
Zone.js. Select a compatible Vite/plugin/Node combination. Next and Nuxt provide their own framework server;
Astro uses a client script inside an Astro page. These fixtures do not certify
every historical framework version, SSR/server failure mode or routing stack.
Build configurations enable source maps for retained stack/source investigation;
keep those maps with the matching owned build instead of publishing them.

Install dependencies without dependency lifecycle scripts, retain the resolved
lockfile, then build, both with the selected absolute pnpm:

```sh
/absolute/bin/pnpm --dir /absolute/owned/project install --ignore-scripts
/absolute/bin/pnpm --dir /absolute/owned/project run build
```

Prepared projects keep dependency lifecycle scripts disabled in
`pnpm-workspace.yaml` and configure pnpm to fail on dependency-state mismatch
before running a script. Build/preview never silently reinstall dependencies;
use the explicit install command to reconcile a mismatch before retrying. Keep
the same CI environment through install, build and preview, since it affects
pnpm's store selection.

Vite development binds loopback on port 4173 with strict port selection. Next
and Nuxt preview accept an explicit port; the acceptance harness selects a fresh
loopback port. Use the actual reported URL and stop
the owned server after the campaign. Each framework owns a component (Astro a
client script), subscribes to fault state and disposes its controller/timers on
unmount. Vite vanilla displays the same state through the DOM. Frame churn makes
at most twelve scheduled updates and updates the framework's output state;
it is not a claim that twelve frames displayed or dropped.

Corrected mode replaces the model, clears held buffers and cancels churn. Reset
does the same cleanup without changing mode. Exception injection is caught by
default; the separate uncaught control requires arming and emits an actual
unhandled promise rejection. Bridge/network actions remain explicitly simulated;
there is no Internet request, connectivity change or native bridge in them.
Preparation/control tests do not certify framework builds or browser execution:

```sh
node --test tools/diagnostic-web.test.mjs
```

For a built project, the explicit browser acceptance entry point selects one
already prepared Chrome cache and one retained CLI artifact. Use a CLI outside
the shared Cargo output directory when other agents may rebuild it. The run
hardlinks that artifact into a new private evidence root and records its digest.
[acceptance-versions.json](web/acceptance-versions.json) contains the selected
direct dependency versions; keep each prepared project's resolved lockfile too.

```sh
node tools/accept-diagnostic-web.mjs /absolute/pinned/testrun /absolute/owned/project /absolute/prepared/browser-cache /absolute/new-private-run /absolute/bin/pnpm
```

This macOS workflow uses isolated headless Chrome through the public CLI. Vite
and Astro production output use an owned static server; Next and Nuxt use their
framework preview server. It verifies actual UI state, a caught and armed
unhandled exception, bounded retention/reset/correction, real loopback HTTP
503/200 responses, genuine V8 CPU capture/import with an explicit 10 ms sampling
interval, reload and retained evidence
after browser close. The same assertion fails for the observed defective buffer
count and passes for the corrected count. Those counts describe fixture-owned
buffers, not a heap census. The fixture's bridge/network buttons remain simulated.

The report preserves CLI/dependency identities and actual operation responses.
Only the run's browser, loopback server, investigation and private manager are
closed; prepared projects, dependency/browser caches and private run evidence
remain. Browser snapshots retain partial-history semantics. Production execution
does not certify HMR, SSR server faults, source-map symbolication, other browser
engines/hosts or causal capture overhead. No computer-use permission is required.

## Prepare framework apps

Choose a **new** absolute root, an installed tool, and exact generator/template
versions. The destination contains an ownership marker, `project/`, and a copy
of the generated dependency manifests. Existing roots are refused, including
partial failed generations. A failure preserves its owned root for inspection.

```sh
node tools/prepare-diagnostic-app.mjs flutter /private/tmp/acme-flutter-diagnostics /absolute/flutter/bin/flutter
node tools/prepare-diagnostic-app.mjs expo /private/tmp/acme-expo-diagnostics /absolute/bin/pnpm SELECTED_CREATE_EXPO_APP_VERSION SELECTED_EXPO_TEMPLATE_BLANK_VERSION
node tools/prepare-diagnostic-app.mjs react-native /private/tmp/acme-rn-diagnostics /absolute/bin/pnpm SELECTED_COMMUNITY_CLI_VERSION SELECTED_REACT_NATIVE_VERSION
```

The arguments are the framework, new root, tool, generator version, runtime or
template version and an optional registry cutoff.

Replace the `SELECTED_*` placeholders with exact `major.minor.patch` versions.
Expo's runtime selection names the **expo-template-blank package version**, not
the Expo SDK number. The generator and template are both selected; generated
manifests record the resulting React/Expo dependencies. Resolve/install and retain
their lockfile before treating a particular build as reproducible.

Expo/RN preparation runs `pnpm dlx` and can acquire the selected generator/template.
It skips app dependency installation. Flutter uses the selected SDK with
`create --no-pub`. There is no EAS account workflow here. No preparation command
builds an app or starts Metro, a simulator, emulator or device.

On macOS, `node tools/setup-diagnostic-flutter.mjs setup` downloads the pinned
Flutter stable archive, verifies its published SHA-256 and the extracted SDK's
version and revision, and precaches Android artifacts into
`~/Library/Caches/testrun/development/flutter` (a further absolute argument
selects another cache). Use its generated `bin/flutter` launcher as the app
tool: it keeps Flutter, Dart, pub, Gradle and Android user state inside that
cache. `node tools/setup-diagnostic-flutter.mjs --remove` deletes only a cache
carrying the setup marker.

`node tools/build-diagnostic-flutter.mjs ROOT TOOL SDK JDK`
installs the fixture sources and native breadcrumb handlers, runs `pub get` and the
widget tests, and builds an x86_64 Debug APK.
`node tools/build-diagnostic-flutter.mjs --ios ROOT TOOL` builds the same sources
as an iOS simulator Debug app instead. The app test replaces the generated counter test and
checks faulty retention, reset and corrected retention. Runtime switches select
corrected mode and the uncaught-error route; `--dart-define=ACME_CORRECTED=true`
and `--dart-define=ACME_UNCAUGHT=true` set their initial values. A fixture
`MethodChannel` writes bounded breadcrumbs from the app process: `AcmeDiagnostics`
logcat entries on Android, `com.example.acme.diagnostics` os_log entries on iOS.
Launched without `flutter run`, a Debug engine starts with Dart profiling off, so
the fixture app enables it itself (an activity shell argument on Android, the
`FLTEnableDartProfiling` Info.plist key on iOS). The Profile workload runs fixed-time
bursts and shows the completed rounds as `acme-profile-rounds`.

```sh
node tools/accept-diagnostic-flutter.mjs /absolute/pinned/testrun /absolute/owned/flutter-root /absolute/android-sdk /absolute/jdk /private/tmp/fla01 8098
node tools/accept-diagnostic-flutter.mjs --ios /absolute/pinned/testrun /absolute/owned/flutter-root com.apple.CoreSimulator.SimRuntime.iOS-26-2 /private/tmp/fli01
node tools/accept-diagnostic-flutter.mjs --cleanup /private/tmp/fla01 /absolute/android-sdk /absolute/jdk
```

The Android form creates a new owned API 36 emulator, waits until
its own post-boot steps finish and its adb transport is stable, reads the app
process's own VM Service announcement, owns one `adb forward` on the final port
argument (8098 above) and a private endpoint file, and releases them with the
emulator. The `--ios` form runs the same ten cases on a new simulator in a
private device set; the simulator app's service listens on host loopback, so only
the endpoint file is owned. `--cleanup` with the run root (and, for Android, the
SDK and JDK) retries either release. Use a short evidence root such as
`/private/tmp/fla01`: long roots exceed the manager's socket path limit.
`--measure` takes the Android arguments and, instead of the cases, alternates the Profile workload with no
capture, an `observe` capture and a `cpu` capture in one app process, recording
rounds, CLI timings and retained state growth.

In the generated Expo/RN `project/`, run `pnpm install` and retain its lockfile.
Both use the hoisted linker required by native tooling. Expo uses its generated
`pnpm exec expo start` / `pnpm exec expo prebuild` workflows; bare RN uses its
generated native Gradle/Xcode projects and Metro workflow. Use the app switches
for corrected mode and explicit uncaught-error injection. Preparation does not
claim those SDK builds or native fault capture passed.

### React Native Android/Hermes acceptance

After preparing an Expo or bare React Native project, select its installed
package manager, SDK, JDK and persistent Gradle cache. Each build refuses
a fixture whose owner marker names the other framework, and checks the generated
Android application ID (`com.example.acme.expodiagnostics` for Expo,
`com.example.acme.rndiagnostics` for bare RN). Building installs dependencies
without dependency lifecycle scripts and compiles only x86_64 Debug; Expo first
runs its Android prebuild, while bare RN uses the Community CLI template's
checked-out native project. Both use local debug signing and no EAS account. The
generated wrapper can acquire its selected Gradle/dependencies; keep the resolved
lockfile and caches. Bare RN preparation passes `--package-name` to the selected
Community CLI; use the CLI version the selected `@react-native-community/template`
pins (20.2.0 for RN 0.88.1).
The build registers the fixture's bounded `AcmeBreadcrumbs` native package,
declared in the generated app's Kotlin package. Its
app-owned JS controls emit explicit `Log.i` breadcrumbs in the native app PID;
the workflow does not assume the SDK forwards JS console output into logcat.
The fixture displays native-bridge availability and requests a fresh native log
before collecting runtime evidence. This verifies one simple logging bridge,
not arbitrary native plugins or their failure behavior.

```sh
node tools/pin-diagnostic-cli.mjs ~/.cargo/bin/cargo /absolute/new-pinned-testrun
node tools/build-diagnostic-react-native.mjs expo /absolute/owned/expo-root /absolute/bin/pnpm /absolute/android-sdk /absolute/jdk /absolute/gradle-cache 8097
node tools/build-diagnostic-react-native.mjs react-native /absolute/owned/rn-root /absolute/bin/pnpm /absolute/android-sdk /absolute/jdk /absolute/gradle-cache 8097
node tools/accept-diagnostic-react-native.mjs /absolute/pinned/testrun /absolute/owned/expo-or-rn-root /absolute/bin/pnpm /absolute/android-sdk /absolute/jdk /absolute/new-run 8097
```

Acceptance selects the framework from the fixture's owner marker and rechecks
its generated application ID. Expo runs `expo start --localhost`; bare RN runs
the project's `react-native start --host 127.0.0.1`. Both prewarm their actual
Android entry bundle and record the framework in the report.

The final argument of the build and acceptance commands is the Metro port
(8097 above); use the same port for build and acceptance. Acceptance refuses an occupied port and owns a new private API 36
Google Play x86_64 AVD, loopback Metro and private Testrun manager. It checks the
running CLI and installed APK digests before actual accessibility input. The
fixture's probe is explicitly enabled for the observed target before Metro bundles
the app; preparation alone leaves it disabled. No arbitrary inspector evaluation,
debugger UI, user phone, root, EAS login or external provider is selected.
`tools/pin-diagnostic-cli.mjs` builds through Cargo's existing cache and hardlinks the
executable named by its artifact message inside that build process. It refuses an
existing destination and avoids assuming an output directory or selecting stale
bytes from another execution context. Pass that pinned destination as the
acceptance CLI; it does not install or replace the host command.
The app launches once, followed by at most 90 seconds of read-only control/probe
readiness checks. Reload similarly requires a newly observed probe epoch rather
than accepting an old ready label. Runtime captures use a ten-second window within
the existing 30-second/8 MiB collector budget; product deadlines are unchanged.
If ready-actor reuse fails its automatic screenshot with `PLATFORM_FAILED`/255,
the harness retains that error and requires fresh ready status before proceeding
to actual native fixture controls. It does not repeat connect or infer screenshot
success; other startup failures stop the run.
For measured actions, the harness resolves the unique native button before
capture, waits for an active runtime window, then taps its measured normalized
position in the unchanged fixture. Slow accessibility lookup does not consume
the recording window.

Checks cover real caught/unhandled JS observations, logical retention/reset/
correction with the same assertion, Hermes CPU sampling, app-declared probe/React
commits, native PID/log observations and explicit app-owned JS reload with fresh
discovery. Bridge/network buttons remain simulations. The bounded profile workload
runs eight 100 ms JS bursts with 40 ms pauses and cancels scheduled work on unmount;
it does not measure user latency or displayed frames. Profile samples and logical
bytes do not establish CPU utilization, heap ownership or GC reclamation.
Native snapshots select app logs without system context and use device-clock
cursors immediately before the explicit native-breadcrumb and retention actions.
They do not rely on arbitrary startup history or certify continuous delivery.

Normal success/failure cleanup stops only the witnessed Metro process group,
after checking that the run's framework matches the app's owner marker,
disables the probe config, closes its investigation and releases/deletes the owned
AVD. CLI/dependency/APK/harness identities, bounded raw responses and private
artifacts remain in the run root. Recovery uses the same script without app input:

```sh
node tools/accept-diagnostic-react-native.mjs --cleanup /absolute/retained-run /absolute/android-sdk /absolute/jdk
```

Metro recovery refuses changed process births and unauthenticated surviving groups.
To verify the query path against a previously retained real caught-error capture,
without starting Metro or a device:

```sh
node tools/replay-diagnostic-react-native.mjs /absolute/pinned/testrun /absolute/retained-run/caught-console.ndjson /absolute/new-replay-root
```

Replay requires an actual retained Hermes probe and checks the imported React
commit/error records through public analysis/query. It retains the original input
digest and private query output, then removes its investigation and retires its
private manager. Replay does not certify a new native/runtime capture.

#### Bare React Native iOS simulator

The same prepared bare RN project also builds for the iOS simulator on macOS.
The prebuilt iOS core compiles in Metro port 8081. Acceptance instead owns Metro on
its final port argument (8097 below) and sets the app's `RCT_jsLocation`
preference on the private simulator before launch, so it never uses another
project's server on 8081. The port must be free on both `127.0.0.1` and `::1`;
the harness follows whichever loopback address its own Metro answers on. Select
installed pnpm and Bundler, a persistent CocoaPods home and a new derived-data
directory:

```sh
node tools/build-diagnostic-react-native-ios.mjs react-native /absolute/owned/rn-root /absolute/bin/pnpm /usr/bin/bundle /absolute/cocoapods-cache /absolute/new-derived-data
node tools/accept-diagnostic-react-native.mjs --ios /absolute/pinned/testrun /absolute/owned/rn-root /absolute/bin/pnpm com.apple.CoreSimulator.SimRuntime.iOS-26-2 /absolute/new-derived-data/Build/Products/Debug-iphonesimulator/AcmeDiagnostics.app /private/tmp/acme-rni1 8097
```

The build installs the template Gemfile's CocoaPods into the project's
`vendor/bundle`, adds the fixture's `AcmeBreadcrumbs` local pod (bounded `os_log`
entries in the app's own process) and compiles an ad-hoc-signed x86_64 or arm64
simulator Debug app; no team, account or host gem installation is involved. If
native gems fail to compile because the newest macOS SDK carries another OS's Ruby
headers, add an installed SDK matching the host Ruby as a final build argument,
such as `/absolute/MacOSX15.4.sdk`; it applies only to gem compilation.

Acceptance creates an iPhone 16 in a private device set (keep the root short for
socket paths), checks every generated bundle identifier, launches with Testrun's
foreground readiness and runs the same nine cases. iOS app logs are an 8-second
window counting back from collection, so each native snapshot follows its action
directly. RN `Text` exposes its label but not its `testID` on iOS, so text slots
are selected by unique exact result text or fixture prefix. Cleanup closes the
target, retires the manager and deletes only the private set's simulator;
`node tools/accept-diagnostic-react-native.mjs --cleanup ROOT` retries that
release for a retained root.

A prepared Expo project uses the same acceptance command after the build
command with `expo` in place of `react-native` and the same arguments. That build runs Expo's
iOS-only prebuild once, adds the fixture's pinned Gemfile (the Expo template has
none) and builds the same way. Expo 58's `expo-modules-jsi` 58.0.21–58.0.22 does
not compile with Xcode 26.3 (Swift 6.2.4). To select an earlier coherent set,
prepare with a final `YYYY-MM-DDTHH:MM:SSZ` registry-cutoff argument: preparation writes
the matching pnpm `minimumReleaseAge` into the project and records the cutoff in
its owner marker, so every package resolves from versions published before it.

Expo Go, Expo iOS, physical/release builds, other selected versions, allocation/retaining analysis, native
bridge/network failures, source-map symbolication, loss and causal overhead require
separate evidence. See [diagnostics verification](../../../../docs/diagnostic-verification.md)
for the tested configurations.

## Select fixture acceptance

Select a newly owned simulator/emulator, this fixture's exact package/build,
collector and symbol identities, allowed triggers, output directory and resource
budget. A fixture app removes the user-app prerequisite; real accounts, physical
hardware, release permissions and SDK/OS coverage remain distinct matrix cells.
Use only its own package for install/relaunch/cleanup. Keep SDK/dependency caches.
Stop owned Metro/tool processes and remove only the disposable target and app
outputs owned by the run. Never fall back to a discovered user's phone/app.

The SDK-free preparation test verifies materialization and ownership with local
generator peers, not the upstream generators or app runtimes:

```sh
node --test tools/prepare-diagnostic-app.test.mjs
```

See the [fault bounds](../README.md) and
[diagnostic verification guide](../../../../docs/diagnostic-verification.md).
