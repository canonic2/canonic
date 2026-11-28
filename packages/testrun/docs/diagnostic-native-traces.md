# Native CPU, scheduling and hang investigations

Use `native_trace` with the existing investigation tools to record or import
Perfetto, simpleperf, xctrace and macOS `sample` evidence. Capture and offline
conversion are explicit operations. Static capabilities never find devices,
launch profilers, install tools or read an input file.

Open an investigation with the same explicit `targetId` and `appId` as the source.
Reuse the returned collector and analysis references; analysis and queries do
not repeat the app journey. Native capture can perturb execution and expose
system/neighbor-process information. No app-overhead measurement or live
device/template certification is established by registration.

## Record or import

For an Android system trace, explicitly authorize its broad capture scope:

```sh
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id native --source '{"kind":"native_trace","backend":"perfetto","mode":"capture","targetId":"android:ACME","appId":"com.example.acme","durationMs":3000,"allowSystemTrace":true,"toolPath":"/absolute/trusted/trace_processor_shell"}' --limits '{"durationMs":60000,"maxBytes":8388608}'
```

Select an installed **native executable**, not a downloader wrapper, for
`toolPath`. Perfetto conversion uses the supported `query --format csv` interface
with fixed read projections. There is no arbitrary SQL, remote trace server,
HTTP listener, automatic acquisition or symbol-server option. Older processors
without that interface produce explicit unavailable sections while preserving
the native artifact.

Perfetto capture defaults to a 2 MiB buffer with `perfettoFillPolicy:"discard"`,
which drops new chunks when full. Set `perfettoBufferKib` from 256 to 3840 and
`perfettoFillPolicy:"ring_buffer"` to keep newer chunks as the buffer fills.
The recording includes scheduler and binder/block events, selected app atrace annotations and process
inventory plus the Android SurfaceFlinger FrameTimeline source. Fixed actual/
expected frame exports preserve UPID, layer and producer jank/present fields;
unsupported Android/processor tables remain gaps. The [performance view](diagnostic-performance.md)
keeps expected budgets and actual durations separate. Kernel/device permissions
determine actual data. System scheduling
and metadata remain broader than the selected package. It does not enable a
native CPU sampler; use simpleperf when CPU call stacks are needed.

For an input-driven capture, connect to the selected Android target before
starting and prepare the `act` batch. Add `waitForPerfettoStart:true` to
the source when the device Perfetto supports `--notify-fd`. Start then waits
for the device's confirmation before replying. Run the prepared batch
immediately on a fresh reply with `perfettoStart.acknowledged:true` and enough
`remainingMsAtReply` for the batch and dispatch overhead. This is the
requested duration minus elapsed host time since recorder dispatch. It can be
zero and does not measure actual time remaining on the device. Keep the same
explicit target on the input request. Inspect and settle an unconfirmed trace
before choosing another submission. A replay returns the collector receipt
without a fresh acknowledgement. Testrun leaves input execution to `act`.

For simpleperf, select `backend:"simpleperf"`, the exact `processId`, and a trusted
host simpleperf `toolPath`. Recording uses device-installed simpleperf,
`cpu-clock:u`, a requested `frequencyHz` (default 99, range 1–1000), DWARF stack
capture capped at 8192 bytes and a unique owned remote file. Android package
selection uses primary user 0. Installed package UID, PID start ticks, boot and
transport are checked before and after recording; shared UIDs and process churn
do not establish exclusive app coverage. No root, run-as, sysctl/property changes,
profiling entitlement changes or binary deployment is performed. Unavailable
profileability/permissions are refusals, never automatic preparation.

For xctrace PID attach, select `backend:"xctrace"`, exact `processId`,
`targetId:"simulator:UUID"`, `"device:UDID"` or `"host:macos"`,
`allowAttach:true`, and one explicit template: `Time Profiler`, `Points of
Interest`, `System Trace`, `Hangs`, `Allocations` or `Leaks`. The
[memory workflow](diagnostic-memory.md) requires separate allocation-recording
or heap-inspection opt-ins for those memory templates. Testrun calls `xcrun xctrace` unless an
explicit trusted `toolPath` names its replacement. It attaches without launching
the app, requesting all processes or accepting prompts. Each template's actual
availability, data schemas and permissions are independent. Device/simulator
birth/app association remains caller-selected and unverified; host PID birth
and executable witnesses are read around attach. Recheck target and PID before
starting. No live template/OS matrix is certified.

### App and render-server CPU on physical iOS

Use `Activity Monitor` for a single recording of the app and `backboardd`.
Select the physical device and the running app's PID. The [iOS diagnostic
source](diagnostic-apple.md#inspect-failures-and-available-installed-state) can
report the app PID when device process inventory is available. Use an
investigation bound to the same target and bundle identifier.

```sh
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id ios-cpu --source '{"kind":"native_trace","backend":"xctrace","mode":"capture","targetId":"device:SELECTED-UDID","appId":"com.example.acme","processId":42,"template":"Activity Monitor","durationMs":5000,"allowSystemTrace":true}' --limits '{"durationMs":90000,"maxBytes":8388608}'
```

MCP `diagnostics_start` takes the same `source` and `limits` objects. Start
collection before driving the interaction. Give the collector time for Xcode
startup, recording and export. `durationMs` inside `source` is the recording
limit in milliseconds. The collector needs at least 16 additional seconds before
starting the recorder and can spend up to another 40 seconds exporting it.

The opt-in records all processes with the named Xcode template. The original
private trace can contain neighboring process data. Structured CPU rows select
the app PID and exact `backboardd` process name from the export. For an externally
recorded `.trace`, select `mode:"import"`, supply the absolute `path`, and keep the
same device, app PID and template fields. Import needs no recording opt-in.

If Instruments ends the run because the device disconnected, the collector
settles as `partial` with reason `device_disconnected`. The source report gives
the recorded duration and Instruments' end reason when available. The retained
trace and its observed samples cover only that interval. An individual export
timeout is reported as a gap. The bounded raw archive remains available if the
collector has time to retain it.

After collection settles, analyze its collector and query the shared trace clock.

```sh
testrun diagnostics analyze diag-RETURNED_REFERENCE collector-RETURNED_REFERENCE --submission-id ios-cpu-analysis
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"view":"timeline","kind":"metric","clockDomain":"native:xctrace"}'
```

Rows give the exported CPU percent, process role, PID and relative timestamp in
nanoseconds. Select the same `from`/`to` time range for both processes and inspect
the reported gaps before comparing runs. Action IDs correlate the trace with the
test. For timestamp alignment, supply an independently established clock anchor
in analysis metadata or choose windows in trace-relative time. The app/PID
association is caller-declared. `backboardd` CPU includes shared system work,
and GPU work and frame hitches have separate measurements.

### Lightweight macOS hang samples

For a lighter hang observation, use `backend:"stack_sample"`,
`targetId:"host:macos"`, exact host `processId` and `allowAttach:true`.
The source invokes macOS `sample` for the requested duration with a 10 ms interval.
Its command can continue after sampling while the native tool processes symbols.
It has a fourteen-second wall-time ceiling, clipped by the collector's remaining
budget; sampling duration is not total command latency. Short UIKit/Swift
recordings can need several additional seconds. The native artifact byte limit,
explicit attach and process-identity checks still apply.
It writes the report to a private owned file and reads that file after the tool
exits. Symbol lookup uses the tool's normal deferred mode; the route does not
request eager `-mayDie` lookup. A process that exits during capture can leave
symbol information unavailable, and process continuity is rechecked separately.
Its inclusive call-graph counts remain overlapping observations, not independent
CPU samples. The pre-attach process state records an already stopped/paused host
process separately; no observed state proves an application freeze or its cause.
Device hang/ANR/crash reports also compose through the existing Apple/Android
report sources. This lightweight route does not attach to physical-device PIDs.

To import an existing recording instead, use `mode:"import"` with an absolute
`path`. Perfetto/simpleperf accept a regular file; xctrace accepts a `.trace`
directory, copied to private owned storage before export. `stack_sample` accepts
a UTF-8 sample report without invoking a profiler. Original inputs are preserved.
Imports retain caller-selected app/target association as unverified metadata.

## Bounds, loss and cleanup

Source duration is 100–10000 ms (default 3000); give the shared collector limit at
least source duration + 5000 ms before recording, plus time for offline conversion.
That is an admission minimum, not a completion guarantee. For `stack_sample`,
leave room for up to fourteen seconds of native sampling/symbol processing plus
identity reads and conversion; a shorter remaining collector budget still wins.
Each retained native artifact is at most 4 MiB and each bundle at most 512
entries. The native collector defaults to `limits.maxBytes:8388608` for the
complete envelope, including the encoded artifact and projections. It rejects
a Perfetto buffer that already exceeds the budget after encoding, and reports
`NATIVE_TRACE_BOUNDS` if the finished envelope exceeds the limit. Use a
smaller buffer or an adequate `maxBytes` and a new submission. Bundle size is checked after the native tool
finishes writing; this is not a live disk quota. Symlinks/special-file inputs are
refused. Large recordings need a shorter scenario or an external reduction.
Scheduling/slices/CPU exports use at most 4096 rows each, process/thread inventory
2048 and statistics 1024; an extra row exposes truncation. Shared parser/index,
row/page and deadline budgets still apply.

Loss/error statistics, unfinished intervals, export limits, missing tables,
unwind gaps and changed process births remain explicit. Empty sections do not
prove absence. Perfetto trace-lifetime UPIDs/UTIDs distinguish reused PIDs/TIDs.
One process profile cannot establish the resource use of a multi-process app.

Cancellation prevents further dispatch and lets the current finite host command
settle through its existing deadline and process owner before removing owned
temporary files. This can take up to the command's remaining deadline (at most
14 seconds). It never stops a global profiler. A finite remote
Perfetto/simpleperf recorder can outlive a cancelled ADB client; remote termination
is unverified. Only the unique simpleperf file is unlinked, after checking the
same transport boot; device loss or later remote creation can leave its cleanup
unverified. Manager/host loss has the same uncertainty. No remote PID is guessed
or signalled and collection is never replayed automatically.

## Query and export retained evidence

After capture settles, analyze the returned collector normally. Query views use
the same retained analysis and filters:

```sh
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"view":"timeline","kind":"span","clockDomain":"native:perfetto"}'
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"view":"call_tree","kind":"stack"}'
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"view":"flamegraph","kind":"stack"}' --limit 20
testrun diagnostics export diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --path /absolute/private/capture.pftrace --filters '{"format":"native_artifact","scope":"all"}'
```

For Perfetto, query `kind:"process"`/`"thread"` first, then use
`nativeUpid` or `nativeUtid` numeric filters to select recorded trace lifetimes,
including that process's known threads, without joining reused OS PIDs.
Timeline sorts one producer clock and refuses to order unrelated clocks without
selection. Perfetto uses its normalized trace nanoseconds; xctrace uses exported
relative nanoseconds; simpleperf retains sample timestamps in producer nanoseconds.
Action IDs associate the scenario but do not create clock anchors or exact causal
intervals. Existing explicit clock-anchor metadata remains separate.

Call-tree rows are inclusive weighted prefixes. Flamegraph rows are exact stack
paths with their recorded weight, suitable for a caller's renderer. Units,
measurement definitions, native identities and recorded thread scopes stay
separate; ancestor/descendant weights must not be summed. Witness ordinals point
back to ordinary rows and their raw source. Inclusive `sample` call-graph lines
are excluded from these aggregations. Missing/unknown frames are explicit.

Scheduling `Running` intervals are on CPU; `R`/`R+` are runnable. Sleep/block
states and producer `io_wait`/`blocked_function` hints are separate evidence.
Locks, IPC/binder, disk/network waits, GC and frame/signpost intervals are exposed
only where the producer recorded them. Names and overlapping intervals do not
establish a wait cause. CPU samples alone never establish off-CPU duration.

Frame names, addresses, mapping build IDs and producer symbols are preserved as
unresolved evidence. Exact retained symbol references can be attached through
existing analysis metadata; references alone do not resolve frames. No native
ELF symbolizer or xctrace symbolicate/download action runs automatically.

Native export writes **original unredacted private artifact bytes**. It requires
`scope:"all"`, `format:"native_artifact"` and no row filters.
An admitted analysis reference is required, but the structured index need not
have succeeded: native export separately verifies retained envelope/artifact
digests, including gzip-wrapped envelopes. xctrace bundles export as `tar` or
`tar.gz`. For Activity Monitor, Testrun can pack up to 16 MiB of trace files
when gzip brings the archive within the 4 MiB retention limit. Testrun leaves
extraction to the caller. Exports are caller-owned,
never uploaded, refuse different existing bytes, and survive investigation
removal. Ordinary exports keep the `minimal-v1` numeric projection default.
