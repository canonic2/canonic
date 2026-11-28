# Verify a diagnostic workflow

Use `diagnostics capabilities` before collecting. Its `verification` inventory
separates shared host fixtures, browser-owned acceptance and unverified native,
framework and provider cells. A parser or inspector peer passing does not certify
actual app capture, release builds, native permissions or a device/SDK version.
Existing UI/build release acceptance has its own owner and is not reopened here.

For a live campaign, select collector host OS/version/architecture, exact
disposable target/transport, app/build/runtime/mode, native tools and symbols,
allowed effects, time/memory/storage limits and cleanup owner. A platform name
alone is insufficient. Production accounts, physical devices and release builds
remain separate selections. Do not discover an arbitrary device or stronger
credential as fallback. Unselected live cells remain unverified.

## Use the Acme source fixtures

Contributor fixtures in `tests/fixtures/diagnostics/` contain Java, Kotlin, Swift,
Objective-C, C++, RN/Expo and Flutter/Dart faults with explicit corrected controls.
The [disposable apps](../tests/fixtures/diagnostics/apps/README.md) provide native
Xcode/Gradle/AppKit/Win32/GTK projects, prepared Expo/bare RN/Flutter entry points,
and Vite vanilla, React, Vue, Svelte, Solid, Preact, Lit, Angular JIT, Next.js,
Nuxt and Astro web apps, so a
campaign does not need a user app. They are not preinstalled or evidence of
compatibility. Native fatal/kill actions affect
only the caller app; hangs last six seconds. Serialize native calls. Do not run
destructive triggers on user apps or as ordinary host tests.

The JS/Dart model limits retention to 1 MiB, work to 100,000 loop iterations per
call, and UI churn to twelve state updates. Reset/dispose drops references and
cancels UI timers. This does not prove GC/ARC collection, display delivery or a
native bridge fault. Plugin/network errors are simulated in-process failures;
they do not change device connectivity or issue external requests. Other runtime
families use their existing native-format/export and protocol fixtures, with
live SDK/engine acceptance remaining separate.

After an intentional exit, use the existing app/project lifecycle owner for an
explicit relaunch. Observe a new process birth/runtime generation; a new PID
alone is insufficient. Fixtures do not auto-restart, kill another process or
enable debugging endpoints. Select debug/profile/release and actual symbols
independently; a corrected mode is not proof that a release failure was fixed.

## Contributor host checks

From the Testrun package:

```sh
node --test tools/diagnostic-fixtures.test.mjs
DIAGNOSTIC_HOST=macos node --test tools/diagnostic-fixtures-native.test.mjs
DIAGNOSTIC_ANDROID_JAR=/absolute/sdk/platforms/android-36/android.jar DIAGNOSTIC_JAVAC=/absolute/jdk/bin/javac DIAGNOSTIC_KOTLINC=/absolute/kotlin/bin/kotlinc node --test tools/diagnostic-fixtures-android.test.mjs
TESTRUN_DIAGNOSTIC_NODE=/absolute/installed/node cargo test --locked --test diagnostic_analysis diagnostic_verification_generated_node_corpus -- --ignored --test-threads=1
```

The first checks the actual SDK-free JS fault model. The explicit macOS check
uses Xcode to compile/run temporary C++/Swift/Objective-C controls, including a
caught Objective-C exception and bounded retained-memory release. It does not
invoke uncaught fatal, kill, hang or background branches. Swift uses main-actor
ownership. Native compilation does not establish iOS or Android acceptance.

The Android source check compiles Java/Kotlin against the explicitly selected
cached `android.jar`, then runs caught/corrected controls on the host JVM using
the selected JDK and Kotlin installation. Sibling `java`/`kotlin` launchers must
exist. It never invokes Android handler, kill or uncaught background branches;
compilation and JVM behavior do not certify ART, ANR or device behavior.
The source check also compiles the fixture's Android activity against the selected
API; it does not package an APK. `node --test tools/prepare-diagnostic-app.test.mjs`
verifies framework app materialization/refusal with owned generator peers. It
does not invoke upstream generators. The iOS and Android fixture build commands
compile the corresponding native fixture apps; the app README gives those
commands, tool inputs, artifact paths and framework preparation.

The Android source check uses SDK Build Tools 36.0.0 `aapt2` alongside the selected
platform JAR to generate real fixture resource IDs before compiling its activity.
`node tools/accept-diagnostic-android.mjs` is the explicit owned API 36 emulator workflow;
its [app instructions](../tests/fixtures/diagnostics/apps/README.md#explicit-android-emulator-acceptance)
describe inputs and effects. Eleven functional cases passed on Android 16/API 36
x86_64/Debug, covering actual
controls, native logs/process generations, retention/correction, partial resource
reads, sleep/recovery and Java/Kotlin/background/self-kill process endings.
Simpleperf produced no profile; physical/OEM/release, other API/ABI combinations,
ANR classification, native unwinding, loss and causal overhead remain unverified.

`node tools/accept-diagnostic-react-native.mjs` has nine passing selected functional cases
for Expo 58.0.23/RN 0.88.1 generated-native Android Debug/Hermes on a new API 36
Google Play x86_64 emulator on the recorded Mac. It checks actual native controls,
caught/unhandled inspector errors, probe/React commits, logical retention/reset/
correction under the same predicate, genuine Hermes CPU sampling, explicit native
PID-scoped breadcrumbs and JS reload with a fresh probe/inspector. Owned cleanup
passed. The same nine cases and owned cleanup passed separately for a bare RN
0.88.1 Community CLI 20.2.0 Android Debug/Hermes app on its own new emulator, and
for the same app as an ad-hoc-signed Debug/Hermes build on a new iOS 26.2 simulator
in a private device set. There, RN text slots are matched by unique text because
iOS does not expose their `testID`. On that simulator they also passed for a dated
Expo 58.0.20/RN 0.88.0 selection prepared with a registry cutoff; the latest Expo
58 patch set does not compile with the recorded Xcode 26.3 (Swift 6.2.4). iOS runs
own Metro on a nondefault port selected through the app's packager preference and,
when Metro listens only on `::1`, an owned loopback forwarder.
The [workflow](../tests/fixtures/diagnostics/apps/README.md#react-native-androidhermes-acceptance)
includes cleanup-only and captured-data replay entry points. Expo Go, the latest
Expo 58 on iOS, physical/release, other RN/Expo/CLI versions,
heap/GC ownership, native plugin/network faults, source maps, loss and causal
overhead remain unverified; native proxy attribution is not authenticated.

`node tools/accept-diagnostic-flutter.mjs` and its `--ios` form have
functional evidence for a Flutter 3.47.9 (Dart 3.13.7) Debug fixture app on a new
API 36 Google Play x86_64 emulator and on a new iOS 26.2 simulator in a private
device set on the recorded Mac. Ten cases passed on each, with owned cleanup:
installed native process and matching VM-reported PID; caught and unhandled Dart
errors from the VM Logging stream; logical retention failure, reset and correction
under the same predicate; a class snapshot with a retaining path through the
fixture state; Dart CPU samples in the fixture workload; own-process native
breadcrumbs; and explicit relaunch with a new VM birth. The harness discovers the
app's own VM Service announcement, writes a private endpoint file and, on Android,
owns one `adb forward`. Its `--measure` form records that app's
throughput with and without an active capture alongside CLI costs and retained
growth. The [workflow](../tests/fixtures/diagnostics/apps/README.md) gives exact
inputs. Profile/release, physical devices, desktop, other
SDK versions, AOT symbolication and causal overhead remain unverified.

The macOS fixture build compiles an AppKit bundle without launching it.
The desktop CMake project builds Win32 or GTK 3 on its actual selected host;
its `acme-controls` CTest selection runs only bounded caught/corrected C++
controls, without GUI dependencies or defective fatal/hang execution. Passing it
on one host does not certify another host's GUI or dump/capture behavior.

The explicit `node tools/accept-diagnostic-macos.mjs` workflow has functional evidence only for
the recorded macOS 15.8.2 x86_64 AppKit fixture with Swift 6.2.4/macOS SDK 26.2,
an optimized app and development CLI. It covers live process snapshots, fixture
byte ownership/correction, one-second native samples, injected main-thread sleep
and recovery, native relaunch generation, fatal/corrected controls and owned
cleanup. Its `ips` case separately checks one explicitly selected real OS report.
AppKit's bounded stdin driver invokes its own buttons; no visual/input automation
is certified. The Objective-C exception trigger's observed SIGSEGV/native-unwind
discrepancy is retained. Short action pairs do not certify causal capture overhead.
The [app instructions](../tests/fixtures/diagnostics/apps/README.md) give exact
inputs.

`node tools/accept-diagnostic-ios.mjs` selects an unsigned Debug UIKit fixture, pinned CLI,
new private simulator set and explicitly installed runtime. The recorded iPhone 16
on iOS 26.2/macOS 15.8.2 x86_64 checks actual accessibility controls, Swift/C++
logical retention/reset/correction, installed executable identity, observed
process birth, real app logs and the same failed/corrected assertion. One-second
host samples fence the exact simulator app PID, retain real stacks during an
injected sleep and verify responsive recovery; relaunch changes native generation.
The separate passing fatal selection retains PTY console breadcrumbs and observed
PID disappearance for Swift, Objective-C, C++ and background faults. Capture and
fatal evidence come from separate runs; the first overall run stopped at its
console assertion. No console helper exit code infers an app signal or IPS report.
Native simulator sampling does not establish physical iOS profiling, complete
heaps, independent symbolication or causal overhead.

`node tools/prepare-diagnostic-web.mjs` materializes a checked-in web app with exact package
versions from an explicit JSON file. It installs nothing and starts no server.
`node --test tools/diagnostic-web.test.mjs` checks controller failure/correction, cancellation,
disposal and every adapter's materialization without SDK/browser dependencies.
The selected pnpm's `run build` builds a prepared app with its installed dependencies.
Each framework owns reactive output and cleanup (Astro owns a client script);
scheduled state churn does not prove a displayed/dropped frame count. Actual
framework build/browser/hydration/error delivery and server capture remain
separate selected acceptance cells.

`node tools/accept-diagnostic-web.mjs` has scoped functional evidence for all eleven
registered production fixtures in isolated headless Chrome on the recorded
macOS host, using the retained development CLI and selected dependency graphs.
Vite/Astro output uses an owned static server; Next/Nuxt use their preview server.
It checks actual UI fault/reset/correction, armed native page errors, the same
logical-buffer assertion, real loopback HTTP failures, genuine V8 CPU import,
reload, retained replay after browser close and owned cleanup. Angular selects
zoneless JIT. CPU sampling uses an explicit interval; it does not establish
capture overhead. Other browser/host/build cells, Zone.js/AOT, HMR, SSR server
failure diagnostics and independent source-map symbolication remain unverified.
The [app instructions](../tests/fixtures/diagnostics/apps/README.md) give the
inputs.

The web, Android and iOS acceptance workflows (default `all` selection) end with the agent workflow over `testrun mcp serve`,
on the run's own retained investigation and before cleanup. It lists the
catalog, reads capabilities and a plan, checks status, pages query and projection
results one row at a time by following `nextOperation`, compares the defective
and corrected measurements and exports JSON and Markdown reports. It collects
nothing and changes no target.

The selected Node corpus check installs nothing. It uses Node's own inspector
to generate a genuine V8 CPU profile, plus actual defective/corrected/insufficient
JS model observations. It imports, parses and queries them through the real CLI
and MCP, deletes original inputs to verify retention, checks the same retained-
buffer assertion, and removes its owned investigations and manager. Insufficient
evidence must remain unavailable. Normalized model observations do not become
RN/Expo native evidence merely because the same model is used in that fixture.

`cargo test --locked --lib diagnostic -- --test-threads=4` selects the existing
host diagnostic contract tests; `--test diagnostic_analysis` in place of `--lib`
selects the relevant real executable flows. Owned peers cover loss/replay,
cancellation, scope/generation changes, full buffers, parser errors, integrity,
privacy, bounded reads and cleanup retry. No mock or reduced artifact certifies
an upstream runtime; use actual producer outputs where available, and name gaps.

## Measure the selected host workload

```sh
TESTRUN_DIAGNOSTIC_REPORT=/absolute/new-measurement.json cargo test --locked --test diagnostic_analysis diagnostic_access_overhead_measurement -- --ignored --test-threads=1
```

This opt-in fixture pins the built executable and uses three repetitions of one
256-event, approximately 272 KiB Chrome JSON import. It measures CLI discovery,
investigation/collector/parser admission, collection/parser settlement, ordinary
status, a bounded raw read, retained query and cleanup. Filesystem walks and
post-call assertions stay outside outer clocks; CLI startup/IPC/JSON decoding and
settlement polling are included. The first investigation can start a cold manager;
later ones use the same manager. Internal collector/parser costs remain separate.

The report records raw/index and retained growth, exact source build identity,
source-read calls, query rows and zero investigation bytes after removal. On
macOS a separate `/usr/bin/time -l` query-child arm observes peak resident bytes;
it is not included in query timing or attributed to the manager, parser or live
heap. Other hosts report that memory arm unavailable. File counts are storage,
not resident memory. Source files survive removal; reports refuse overwrite.

Correctness, declared byte/row/event limits and owned cleanup are requirements.
Measured times are diagnostics under the recorded workload, not invented shipping
thresholds. Three fixed-order samples on a shared development host do not establish
tail latency, CPU attribution, target-app overhead, live collector event loss or
cross-host/release performance. A capture overhead claim needs matched controls
on its explicitly selected app/target; do not subtract these unrelated arms.
