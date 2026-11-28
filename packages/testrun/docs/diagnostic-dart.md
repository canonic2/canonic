# Flutter and Dart investigations

The `dart` diagnostic source uses an existing Dart VM Service through the shared
investigation tools. It supports a fixed read subset of negotiated protocol
major 4. Registration does not certify an SDK/build/platform range or native
device association. It never starts Flutter, forwards a port, disables service
authentication, changes profiler/logging settings, resumes, reloads or evaluates
application code.

## Credentials and discovery

Save the exact authorized VM Service WebSocket URI in an absolute regular file.
On Unix it must be owned by your user with no group/other access, normally mode
0600. Select `endpointFile`; keep the URI/token out of CLI arguments and reports.
The URI must use a numeric loopback address and explicit nondefault port, with
the shape `ws://127.0.0.1:PORT/TOKEN/ws`. HTTP discovery, DNS, redirects, proxies,
query credentials and remote hosts are refused. Establish forwarding separately.
An intentionally unauthenticated `/ws` route additionally needs
`allowUnauthenticated:true`; Testrun never disables authentication itself.
A local lease admits one Testrun capture per service port. It does not establish
exclusive ownership against external debuggers.

Open an investigation and discover the exact service and isolate:

```sh
testrun diagnostics open --submission-id flutter-case --question 'Investigate Acme frame and retained objects' --binding '{"targetId":"android:ACME","appId":"com.example.acme","actionIds":["action:acme"]}'
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id vm --source '{"kind":"dart","endpointFile":"/absolute/private/service-uri","targetId":"android:ACME","appId":"com.example.acme","runtimeKind":"flutter_native","mode":"discover"}'
```

Read the returned collector with `diagnostics read`. Discovery returns VM
`startTime`, PID/version and an isolate inventory; it does not subscribe or
allocate inspector objects. Discover again with an explicit `isolateId` to read
that isolate's `startTime`, root library, group and registered extension names.
Select the intended isolate, never the first one by default. VM/isolate start
times are producer-reported birth observations, not proof of an OS app/device.
A live VM can report them 1 ms apart between reads, so captures accept the
selected births within 2 ms; the VM PID, isolate ID, root library and group must
match exactly. A different VM or a restarted isolate is still refused.
The ordinary status response omits the URI. Known service URI/token matches are
redacted in captured keys and string values; other application secrets can remain
in private evidence. Imported artifacts are not automatically authenticated or
credential-sanitized.

`runtimeKind` distinguishes `flutter_native`, `flutter_desktop` and `dart_cli`.
Native uses existing `android:`, `device:` or `simulator:` bindings. Desktop and
CLI diagnostics use explicit caller labels `desktop:ACME` and `dart:ACME`; these
do not create general desktop input automation or discover a process for you.
Flutter web uses existing browser evidence; this adapter does not assume native
VM heap APIs exist through DWDS. Flutter release commonly lacks a service;
debug/profile/release and optional build IDs remain declared, with each method's
actual availability recorded independently. No missing endpoint proves that
native evidence or a defect is absent.

## Capture, query and compare

Use the selected values from discovery. A Flutter app started without `flutter run`
starts with Dart profiling off, so `getCpuSamples` reports the method unavailable;
the app enables it with the `--enable-dart-profiling` engine argument (Android) or
the `FLTEnableDartProfiling` Info.plist key (iOS). Testrun never changes it.

```sh
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id flutter-runtime --source '{"kind":"dart","endpointFile":"/absolute/private/service-uri","targetId":"android:ACME","appId":"com.example.acme","runtimeKind":"flutter_native","isolateId":"RETURNED_ISOLATE","vmStartTime":1000,"isolateStartTime":1001,"mode":"cpu","sampleMs":1000,"inspector":true,"network":true,"timeline":true,"buildMode":"profile","buildId":"EXACT_BUILD_ID"}'
```

Replace the example start times as well as the isolate ID. `observe`, `memory`
and `inspect` collect the same core observations; `cpu` additionally reads existing
CPU samples for the observation window, which opens after the before snapshot and
object reads. Debug builds sample densely: one second of a small Debug Flutter app
was a 17 MB reply. Testrun keeps compact functions and groups samples by exact
leaf-first stack with counts (at most 4,096 stacks, 128 frames and 30,000 bytes per
stack); a cut adds `getCpuSamples:stacks_or_frames_limited`. Keep Debug CPU windows
short (`sampleMs` near 1000). A reply above 32 MiB closes the connection with
`DART_RESULT_BOUNDS` and its size. All capture modes subscribe to
isolate/debug/logging/GC/extension events and take bounded current/async stack,
heap/external-memory and allocation-class snapshots before and after the window.
Heap, class, instance and retaining reads address the selected isolate but can
cover its shared VM isolateGroup, including sibling-isolate objects. They do not
establish per-isolate or native-app memory ownership.
They do not enable CPU sampling, force GC, reset allocation counters or change
timeline recorder flags. `sampleMs` defaults to 1000, range 100–10000, after
setup. The shared duration/byte/chunk limits still apply. Service inspection can
pause or perturb the application; no app-overhead guarantee is established.

`inspector:true` reads available root-widget diagnostics, a depth-one details
subtree, creation-tracking availability and rebuild/repaint tracking state.
Object-bearing reads require the advertised group-disposal extension. They own
a unique inspector group, which is disposed during normal settlement. They never
enable tracking, force a rebuild/repaint, change selection or capture screenshots.
Existing Flutter frame and rebuild/paint extension events remain available as
structured evidence when emitted. Frame build/raster/span durations are producer
microseconds; raster-thread work is not GPU execution or displayed-frame latency.

`network:true` reads existing `dart:io` HTTP-profile state and request references
when advertised. Logging is never enabled or cleared, and request/response bodies
are not requested. Native plugins, platform network libraries and web traffic may
be absent. `timeline:true` reads the already-recorded VM-wide timeline; it is not
app-isolated. `processMemory:true` reads whole-process buckets, which may overlap.
`includeVmOutput:true` explicitly includes VM-wide stdout/stderr that may lack
isolate attribution. Missing/denied methods stay unavailable without fallback.

After settlement, analyze and query the returned collector:

```sh
testrun diagnostics analyze diag-RETURNED_REFERENCE collector-RETURNED_REFERENCE --submission-id parse
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"kind":"failure"}'
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"kind":"frame"}'
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"kind":"retention"}'
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"kind":"allocation"}'
testrun diagnostics compare diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE
```

The last example returns `observedDartSnapshots` for compatible before/after
class statistics inside the same collector epoch. Fresh captures have different
epochs and are not silently compared across hot reload. Exact VM/isolate births,
root/class identities, accumulator-reset identity and ordered VM clocks are
required. Differences do not establish object matching, retained size or a leak.
Whole-analysis compatibility retains its stricter existing rules independently.

Rows preserve raw witnesses and source units. Collector time, VM monotonic CPU/
timeline time, event wall timestamps and Flutter frame times are not aligned
automatically. Physical `appId` row scope stays unknown; default app-only redacted
projections omit it. Deliberate broad projections use `scope:"all"`.

## Inspect retained objects

Each `allocation` row preserves a class's current/accumulated byte and instance
statistics with explicit units; its scalar value is current bytes. This avoids
expanding one class into four repeated rows.

A Debug Flutter isolate's allocation profile lists every loaded class, most with
no instances; a small app reports about 5,800 classes in a 5 MB reply. Each
snapshot keeps the classes with any nonzero counter, at most 1,024: classes with
current instances first, then by current and accumulated bytes. The snapshot
records `membersReported`, `membersOmittedWithoutCounters` and
`membersOmittedByLimit`; a cut adds the source loss
`getAllocationProfile:classes_limited_to_1024`. A class kept in only one of the
before/after snapshots is not compared.

Query the allocation snapshot's class identities, then select up to four
`classIds` in a new `memory` or `inspect` capture with the same observed VM/isolate
births. The collector reads at most 16 instances per selected class and inspects
at most four fresh returned instance IDs in total in that window. It obtains
bounded object fields, retaining paths and inbound references. Optional
`objectIds` selects at most four additional exact temporary IDs; collected or
expired IDs remain unavailable and are never replaced with another object.

Paths have a 32-element limit; long paths may be truncated at the root. Object
and stack/detail results above 32 KiB become explicit unavailable observations.
Inspection itself and inspector groups can affect object lifetime. A reported
root/field is evidence of that observed path, not a leak conclusion. Dart heap
usage, heap capacity, embedder-reported external bytes and process buckets stay
separate. This route supplies class snapshots and live retaining queries; full
binary heap snapshot capture, dominators and cross-snapshot object matching are
not registered.

## App size, AOT and native boundaries

Import an existing Flutter `*-code-size-analysis_*.json` as
`format:"flutter-app-size-v1"`. Testrun analyzes its `n`/`value`/`children` tree
for apk/aab/ios/macos/windows/linux. Parent sizes include descendants and must not
be added together; producer compression/package semantics remain explicit and
do not establish installed/download size. Build generation is a separate existing
build workflow, never automatic diagnostic preparation.

An optional wrapper binds declared symbol identity:
`{"schemaVersion":1,"buildId":"EXACT_BUILD_ID","architecture":"arm64","sizeAnalysis":{...}}`.
Import matching Dart debug-info artifacts separately and select their exact
retained digests/build/architecture through existing analysis symbol metadata.
References remain unresolved; Testrun does not run `flutter symbolize`, parse
binary AOT snapshots, infer symbol matches from filenames or use the current
checkout's symbols. Obfuscated/native frames remain raw where identity is absent.

`diagnostics_capabilities.dartProbe.source` ships an optional app-owned Dart
template. Integrate and test it explicitly with the app SDK. Its bounded
`mark` and `measure` emit `Testrun.Diagnostics` extension events with action IDs
and declared app/build metadata. For example, wrap an owned channel call in
`probe.measure('channel operation', () => channel.invokeMethod('load'), actionId: 'action:acme')`.
The template preserves application results/errors and stops emitting after
`dispose()` or its event budget; it registers no global extension or handler.
It has no Flutter dependency and is not installed/executed by Testrun. It requires
validation with the consuming app's Dart SDK; fixture checks do not certify it.

Collect Android/Apple evidence alongside Dart observations with the same action
IDs. Native plugin crashes, GPU faults and OS termination can occur without a
Dart exception; a channel duration or nearby log is not causal proof.

## Interruption and cleanup

Reload, restart, selected-isolate exit/pause, protocol loss or an uncertain RPC
fences reuse. Unattributed reload events fence conservatively. Testrun never
reconnects or resumes; interruption packets preserve already-observed witnesses.
Normal finish, cancellation and capture limits dispose the owned inspector group
once, close the socket and release the lease before terminal status. Timeouts,
reload or hard manager loss can leave inspector handles unverified; status reports
that explicitly. Socket-owned subscriptions end with closure, without global
stream resets. Existing native/browser sources retain independent ownership.

Owned peer/parser/CLI fixtures verify these contracts. Selected Flutter 3.47.9
Debug fixture apps passed live acceptance on an API 36 x86_64 Android emulator and
an iOS 26.2 simulator.
Desktop/phone/profile/release/background behavior, other SDK versions, AOT
symbolication and general target overhead remain unverified.
