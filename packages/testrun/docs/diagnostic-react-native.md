# React Native and Expo investigations

Use `diagnostics_capabilities` to inspect the `react_native` source before
collecting. It uses an explicitly selected Metro inspector HTTP endpoint and
the shared investigation lifecycle. It does not open DevTools or start Metro.
Bare RN, Expo development clients and Expo Go use the same protocol route when
their runtime exposes it. Availability remains unknown until selected.

## Select the runtime

Open an investigation bound to the native `targetId` and `appId`, then discover:

```sh
testrun diagnostics open --submission-id rn-case --question 'Investigate Acme action' --binding '{"targetId":"android:ACME","appId":"com.example.acme","actionIds":["action:acme"]}'
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id discover --source '{"kind":"react_native","endpoint":"http://127.0.0.1:8081","targetId":"android:ACME","appId":"com.example.acme","mode":"discover"}'
```

Read the returned collector through `diagnostics read`. Discovery performs only
bounded `/json/list` HTTP reads, filters the exact proxy `appId`, and returns
`id` and `logicalDeviceId`. These are logical inspector identities, **not a
phone serial number or proof of installed build identity**. Multiple matching
pages require an explicit selection. Missing pages can mean a disconnected,
backgrounded, restricted or unsupported build; they do not prove missing native
evidence. Expo Go's proxy app ID identifies its host, not an individual project.
Use app-declared `projectId`, bundle/update metadata and separate native evidence
to distinguish it. Never substitute a development client's package automatically.

Only numeric loopback HTTP with an explicit nondefault port and base path `/`
is accepted. No DNS, proxy, redirect, port scan or forwarding is performed.
Establish any remote forwarding separately and select its loopback endpoint.
Authenticated/path-bearing inspector routes are unavailable through this source.
The inspector upgrade supplies the selected endpoint's loopback HTTP origin for
Metro's origin validation. It does not disable that validation or accept arbitrary
origin/header overrides.

## Capture and analyze

Use the exact returned page and logical device values:

```sh
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id runtime --source '{"kind":"react_native","endpoint":"http://127.0.0.1:8081","targetId":"android:ACME","appId":"com.example.acme","pageId":"RETURNED_PAGE_ID","logicalDeviceId":"RETURNED_LOGICAL_DEVICE","mode":"logs","network":true,"scripts":true}'
```

`logs` attaches and enables Runtime and Log observation. `network:true` enables
inspector-visible requests; native libraries, images and modules can be absent.
`scripts:true` enables debugger script metadata, which can perturb execution.
Script hashes, source-map URLs, console arguments, exception details and async
parents are retained when reported. Maps are not downloaded or resolved.
Unhandled rejections depend on what the runtime emits. A JS exception mentioning
a native module does not establish its native cause; collect Android or Apple
evidence in the same investigation with the same action IDs.

`mode:"cpu"` and `mode:"heap_sampling"` require both `probe:true` and
`allowProfilerControl:true`. A fixed JS expression checks Hermes and reads an
optional preinstalled probe. No arbitrary expression/method field is accepted.
Coordinate with other debuggers: these profiler domains are runtime-global,
and inspector multiplexing does not establish exclusive profiler ownership.
Rejected starts never cause a borrowed profiler to be stopped. A timed-out start
has uncertain effects; explicit profiler control authorizes one stop attempt.

CPU analysis returns recorded leaf stacks and sample counts, preserving optional
producer microsecond deltas and windows. Counts are not CPU utilization or
native/off-CPU time. Heap sampling uses a 32 KiB interval and depth 32, returning
allocation `selfSize` estimates in bytes. It supplies no retaining paths, full
snapshot, live-heap or leak conclusion and never uses the browser V8 decoder.

`sampleMs` defaults to 1000, range 100–10000, after setup; the investigation's
duration/byte/event limits still apply. Logs/profile work is partial, and may
perturb development execution. It has no measured app-overhead guarantee.

After settlement, run `diagnostics analyze` on the returned collector and use
`diagnostics query` for `failure`, `stack`, `metric`, `marker` or `span` records.
The retained format is `react-native-diagnostics-v1`; it can also be imported.
Rows preserve raw line witnesses, producer metadata and caller action links.
Runtime timestamps stay producer fields; collector wall time does not establish
clock alignment. Default app-only projections omit these rows because physical
app attribution is unverified; deliberate broad projections use `scope:"all"`.
Raw `diagnostics query` filters select rows, for example
`--filters '{"kind":"failure","limit":100}'`; they do not accept `scope`.
The `scope` selector belongs to the redacted `diagnostics project`/`export` APIs.
Failure rows include producer-reported exceptions and error-level console/log
entries. Their original event type remains available; an error log alone does
not establish an uncaught exception, native crash or cause.

## Optional app-owned markers and React commits

Retrieve the CommonJS helper from `reactNativeProbe.source` in the
`diagnostics_capabilities` MCP response. Save it as `react-native.js` in the app's
diagnostic build when you intend to instrument that app, then initialize it
for the intended window.

```js
const {installDiagnosticProbe} = require('./react-native');
const probe = installDiagnosticProbe({
  appId: 'com.example.acme', targetId: 'android:ACME',
  buildMode: 'development', clientMode: 'development-client',
  buildId: 'BUILD_ID', bundleHash: 'EXACT_BUNDLE_HASH',
  sourceMapHash: 'EXACT_MAP_HASH', updateId: 'UPDATE_ID',
  runtimeVersion: 'RUNTIME_VERSION', projectId: 'PROJECT_ID',
});
probe.mark('action started', 'action:acme', {state: 'loading'});
// Render: <Profiler id="Acme screen" onRender={probe.onRender}>…</Profiler>
// Around an owned bridge/module operation, using the same performance clock:
const start = performance.now();
// ... operation ...
probe.measure('module operation', start, performance.now(), 'action:acme');
// After the final snapshot has been collected:
probe.uninstall();
```

Set `probe:true` on collection to read it. Testrun never edits the app, installs
the helper or attaches React DevTools automatically. Metadata is app-declared,
not authenticated; app/target mismatches fence profiling. Optional version fields
include `reactNativeVersion`, `hermesVersion`, and `expoSdkVersion`.
The ring retains 64 records, reports drops, accepts at most eight bounded scalar
fields per marker, and returns copies. Uninstall removes only its owned global.
Explicit install/use/removal adds instrumentation work; business logic must not
depend on it. This helper has no React dependency.

`onRender` records commits reported by React Profiler, including actual/base
duration, phase, start and commit times. Its component identity is the supplied
Profiler ID scoped to the probe installation epoch. It does not enumerate all
components, capture abandoned render attempts or explain why a component rendered.
Strict Mode, development execution and profiling-build availability affect the
observations; native frame timing remains separate. Repeated ring snapshots
deduplicate sequence IDs within the same probe generation.

## Contention, reload and cleanup

An inspector that does not advertise multiple debuggers refuses attachment unless
`allowDebuggerTakeover:true` explicitly permits disconnecting its existing
debugger. Synthetic reload-retargeting page `-1` is refused. Observed context
clear/replacement, pause, detach or changed script hash fences the window. Testrun
never reconnects, retargets, reloads, steps or resumes the app. A proxy can silently
rebind without sufficient identity events; the socket epoch is not proof against
that behavior. Probe installation generations and final discovery checks provide
additional observations, not native identity authentication.

Normal finish, cancellation and capture limits settle before terminal metadata:
one owned profiler stop attempt, socket closure, local lease release. Runtime
replacement or manager/transport loss can leave native profiler state unknown;
inspect the original runtime before starting another profiler. Status reports
this uncertainty and never claims cleanup after hard manager loss. Endpoint and
captured content stay out of ordinary status, but raw evidence can contain secrets.

Owned fixtures verify the protocol subset and parser, including unavailable
endpoints and contention. No live RN/Expo version range, release/profile-build,
phone, background-mode or external-debugger acceptance is certified. Restricted
builds use the existing native logs/report sources when JS inspection is absent.
