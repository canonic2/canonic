# Acme diagnostic faults

These are app-owned fault sources with [disposable fixture apps](apps/README.md)
for native iOS/Android/macOS/Windows/Linux, Expo, bare React Native, Flutter and
eleven web stacks (Vite vanilla, React, Vue, Svelte, Solid, Preact, Lit, Angular,
Next.js, Nuxt and Astro). The native projects
are checked in; the framework preparation target combines selected upstream
templates with these entry points. They are not preinstalled apps or captured
production evidence. Mount/call faults only in the selected disposable app/build.
The fault sources never download SDKs, discover targets, enable debug ports, change device
settings or issue real network requests. `corrected` selects the bounded control.
No source-only or host-model check establishes mobile/native compatibility.

| Source | Explicit defective trigger | Corrected / reset | Remaining prerequisite |
| --- | --- | --- | --- |
| Android Java/Kotlin | Uncaught exception or background thread exception; posted main-thread sleep; self process kill | No exception/sleep/kill; reset drops held bytes | Caller supplies disposable Android app, source set and Java/Kotlin build; background exceptions can terminate the app |
| Swift | `fatalError` on selected current/background thread; posted main-thread sleep; retained Data | No fatal/sleep; `reset` releases bytes | Caller owns Apple app, SDK, signing, build and actor/thread context |
| Objective-C | Selected NSException, background exception or main-thread sleep | No exception/sleep | Link Foundation and include header in disposable app |
| C++ / JNI / FFI | `abort` or synchronous sleep; held native bytes | No abort/sleep; corrected retain clears bytes | Explicitly link and bind exported functions into caller's owned native app; no bridge generator is supplied |
| RN/Expo | JS error, bounded expensive work, retained arrays, frame-state churn, simulated bridge/network rejection | Ready/200, less work, release on reset/dispose | Mount `AcmeDiagnostics.jsx` beside `runtime.js` in owned RN/Expo app; native SDK/build/runtime scope remains external |
| Flutter/Dart | Dart error, bounded expensive work, retained arrays, frame-state churn, simulated plugin/network rejection | Ready/200, less work, release on reset/dispose | Copy Dart sources into owned Flutter project; caller selects actual SDK/build and widget arguments |
| Native desktop | AppKit Swift/Objective-C/C++ or Win32/GTK C++ errors, bounded CPU work, retained bytes and armed hang/abort/background actions | Corrected controls, explicit reset/ordinary-close release | Build the selected host app in `apps/`; Win32/GTK build/runtime coverage is separate from shared controls |
| Web | Browser JS error/rejection, bounded CPU/retention and scheduled DOM/framework state updates | Corrected model, reset/unmount cancels scheduled work and drops buffers | Materialize one checked-in web adapter with selected exact dependency versions; no native bridge/real network fault is implied |

Native fatal and self-kill branches intentionally terminate the app. Hangs block
only the selected thread for **six seconds**, not indefinitely. An OS ANR/watchdog
report is conditional on target state and native thresholds; sleep alone does not
prove one. Retention holds at most **1 MiB** per fixture owner, replaces prior held
buffers, and has explicit reset/release. Concurrent native calls are outside the
fixture contract; serialize actions. Do not invoke fatal/background/kill branches
as ordinary host unit tests or on a user's running app.

The JS/Dart models perform at most 100,000 loop iterations, twelve frame-state
ticks and 1 MiB retained bytes. Churn creates requested UI state work, not a claim
about actual dropped/displayed frames. Bridge/plugin/network errors are injected
in-process outcomes, not native plugin failures, device offline state or external
HTTP. The RN component catches errors by default; explicit `uncaught` switches the
exception action to an unhandled promise path. Flutter's default UI catches and
shows the thrown Dart error. Use the actual app error/probe owner when measuring
its runtime error delivery. Disposing the component/widget cancels its timers and
releases references; this is not proof of native GC/ARC collection.

The Flutter entry point accepts build-time `ACME_CORRECTED` and `ACME_UNCAUGHT`
Dart defines. Rebuild/restart with `ACME_CORRECTED=true` for the control; explicit
`ACME_UNCAUGHT=true` sends the exception action to Flutter's application error
handler instead of the fixture's display catch. This does not guarantee process
termination. The fixture-scoped package manifest only declares JS module syntax;
it installs no dependencies and is not a product package.

Relaunch uses the existing explicit app/project lifecycle owner after observing
exit. Capture process birth/runtime generation before and after, and confirm that
they changed. These fixtures do not launch, restart or signal another process.
Do not turn a corrected self-kill into a successful relaunch assertion.

Run `node --test tools/diagnostic-fixtures.test.mjs` from the package for the
SDK-free JS model contract.
`DIAGNOSTIC_HOST=macos node --test tools/diagnostic-fixtures-native.test.mjs`
exercises nonfatal native controls. The explicitly configured Android source check in
the [verification guide](../../../docs/diagnostic-verification.md) checks Java/
Kotlin compilation and caught/corrected JVM controls without device execution.
Other framework/service/engine adapters retain their native-format and
owned protocol fixtures in `src/diagnostics/`; they do not inherit executable
source-fault or live engine acceptance from these apps.
