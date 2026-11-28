# Inspect Android state and resource series

Use `android_state` for read-only snapshots or repeated sampling of a selected
installed application and Android user. It uses the same authorized ADB transport,
boot and process-generation checks as [Android crash diagnostics](diagnostic-android.md).
It does not collect logcat or historical reports. Android API 26+ and an already
installed ADB are required; individual reads remain permission/OEM dependent.

```sh
testrun diagnostics open --submission-id acme-resources --question "What changed around this action?" --binding '{"targetId":"android:SELECTED-SERIAL","appId":"com.example.acme","actionIds":["action:EXISTING_REFERENCE"]}'
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id before --source '{"kind":"android_state","targetId":"android:SELECTED-SERIAL","appId":"com.example.acme","userId":0,"groups":["process","memory","power"],"label":"before"}'
testrun diagnostics status diag-RETURNED_REFERENCE
```

Keep the returned collector ID. The investigation binding must match the source's
target and app. `userId` selects the Android user/profile, not a Linux UID.
`actionIds` are caller-declared investigation context; collection neither performs
nor verifies those actions. They appear as queryable correlations, without
asserting that an action caused a resource change.

## Select the work

| Group | Observations and limits |
| --- | --- |
| `process` | `/proc` RSS categories, state, thread count, CPU ticks/time, I/O counters, descriptor count and up to 128 thread states per selected process |
| `memory` | Selected PID's `dumpsys meminfo -s` summary: category PSS, total PSS/RSS/swap PSS where recognized; original bounded output retained |
| `scheduling` | Refreshed service/foreground-service fields and bounded selected app/user job and alarm records; WorkManager's private graph is not inferred from JobScheduler |
| `power` | Device battery/charging, Doze/idle, thermal status and wakelock context; app standby bucket and background app-op mode on API 28+; manufacturer with unknown OEM-specific policy |
| `frames` | Selected PID's recent HWUI `gfxinfo framestats` history, with valid completed-frame durations and bounded deduplication; no reset |
| `connectivity` | Existing network readback and parser: observed switches and default-network availability; device context, without `network_set` or switch changes |
| `media` | Optional fixed audio/media-session dumps; only exact app plus selected user/UID lines, without assigning following unscoped fields or starting probes/helpers |
| `device_context` | Optional device locale/timezone properties and selected-user font-scale/user-rotation/auto-rotation settings get; no setters |

The default groups are `process`, `memory`, `power`. Supply distinct names to
select a different set. Options are `mode: "snapshot"` (default) or `"follow"`,
`intervalMs` 500–60000 (default 1000), `maxProcesses` 1–8 (default 4), and an
optional label of at most 256 bytes. Shared collection duration/byte/chunk limits
still apply; default duration is 30 seconds. Follow waits the selected interval
after completing each cycle, so this is not a fixed-frequency sampler.

```sh
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id repeated-action --source '{"kind":"android_state","targetId":"android:SELECTED-SERIAL","appId":"com.example.acme","userId":0,"groups":["process","memory","frames"],"mode":"follow","intervalMs":1000,"maxProcesses":2,"label":"repetition-series"}' --limits '{"durationMs":60000,"maxBytes":8388608}'
```

Run the separately authorized reproduction through Testrun's existing action
tools, then stop the collector and poll until settled. Source collection sends
no app input and does not launch, reset or relaunch the app.

```sh
testrun diagnostics stop diag-RETURNED_REFERENCE --collector-id collector-RETURNED_REFERENCE
testrun diagnostics analyze diag-RETURNED_REFERENCE collector-RETURNED_REFERENCE --submission-id parse-resources
testrun diagnostics analysis-status diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"kind":"metric","correlationId":"action:EXISTING_REFERENCE"}' --limit 100
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"kind":"thread"}'
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"kind":"event"}' --limit 100
```

Follow pagination. Metric rows include units, definitions, PID/generation, label,
counter window, action context and exact packet witnesses. State events expose
typed fields and per-reading status; raw reads preserve private underlying output
where retained. Capture is always partial. Denial, unknown format, process loss,
missing fields and unsupported routes never become zero measurements or proof
of healthy state. A PID changed during a process sample invalidates that sample.

## Interpret measurements

RSS includes shared resident pages; PSS apportions sharing. Meminfo's Java/native
heap PSS categories are resident-memory accounting, not live allocation or
retaining-path measurements. Live heap ownership, allocator retention and GC
count/pause fields remain unknown. This source does not force GC or create heaps.

CPU time uses observed `CLK_TCK` and cumulative process user/kernel ticks, excluding
child-process accounting. Utilization needs two readings from the same process
generation with increasing boot uptime and non-regressing counters. It is CPU
seconds divided by that interval: 100% means one core and values may exceed 100%.
Sequential reads introduce timing uncertainty. Missing clock frequency produces
ticks without guessed CPU seconds/utilization. I/O separates syscall bytes and
counts from storage-accounted bytes; these are cumulative counters, not disk
latency. Descriptor counts come from numeric directory entries, without reading
their targets.

Frame duration is `FrameCompleted - IntendedVsync` for accepted `Flags=0` rows,
in milliseconds. Original nanosecond values retain their monotonic clock and
exact decimal strings. They are recent history, not a guaranteed complete capture
window, deadline-based jank detection or CPU execution time. Epoch row timestamps
describe the collection cycle; they do not align native frame clocks. Histories
are deduplicated within the collector, capped at 32768 keys; loss/caps remain
explicit. No `gfxinfo reset` is sent.

## Compare before and after

After the action, start a new snapshot with a new submission ID and `label:
"after"`. Analyze both collectors, then:

```sh
testrun diagnostics compare diag-RETURNED_REFERENCE analysis-BEFORE_REFERENCE diag-RETURNED_REFERENCE analysis-AFTER_REFERENCE
```

The existing whole-capture `compatible` check stays conservative about incomplete
observations. `observedAndroidState` separately compares the last scalar
process observation in each nonoverlapping collection when packet target/app/user/UID,
boot, installed version/update, process generation, clock frequency, metric
definition and unit match. It includes before/after witnesses and a delta, up to
200 groups. Missing identity, changed generations, unknown timing, overlapping
windows or changed parsers produce no matched difference. Both collectors must
finish their snapshot sequence (or complete an explicit import); stopped,
identity-lost or bounded-off collectors can retain series but cannot establish
the closing package check for this comparison. Use `process` alongside
`memory` to retain the required uptime window. Frames are excluded from this
scalar comparison because their recent-history windows can overlap.

For repeated actions, inspect the series and observation windows, distinguishing
warm-up/caching from continued growth. A rising RSS/PSS series does not establish
retained ownership or a leak; neither a plateau nor a decrease proves absence.
Retaining paths require a separately supported heap source. Power, thermal,
standby, app-op and OEM context are possible constraints, never causal proof of
a disappearance or performance failure.

## Reuse media/audio and respect scope

State coverage and capabilities point to the existing `media_status`,
`volume_get` and `audio_probe` tools. For a prepared connected Android target,
use `testrun media status --target android:SELECTED-SERIAL` through that playback
owner. State collection does not prepare its helper, duplicate its parser or
automatically start audio capture. `audio_probe` retains its separate routing
consent and effects. See [playback](playback.md).

Power and connectivity describe the device. Jobs/alarms/services may read broader
system output before host filtering; retained selected records require exact
package and user/UID association. Package metadata can include other users.
Private proc/meminfo output can contain app details; raw bytes and full queries
remain explicit private-content operations. Numeric projections still follow
[access/privacy](diagnostic-access.md). No rooting, settings writes, switch changes,
helper installs, forced collection, uploads or OEM security bypass occur.

Every ADB client has at most four seconds or the remaining collection duration.
Stop also interrupts long between-cycle waits. Existing target release, manager
drain, replay and evidence preservation apply. Native device/OEM/Android-version
acceptance, thermal sensor variations and capture overhead remain unverified;
owned fake-ADB tests establish the host contract, not target performance.
