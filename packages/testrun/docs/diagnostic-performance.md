# Startup, rendering and UI performance

Use `testrun diagnostics capabilities` and its `performance` catalog to choose
an available source. The shared `performance` view reads retained evidence;
it does not launch an app, reset caches, enable framework instrumentation or
start a GPU capture. Keep the selected device, app/build, build mode, process
generation, workload, refresh rate, warmup and instrumentation in the evidence.

## Choose a source

| Question | Source and limits |
| --- | --- |
| Android frame completion | Existing `android_state` source with `groups:["frames"]`; selected-process recent HWUI history, not every displayed frame. No history reset. |
| Android frame budget/jank | Existing `native_trace` Perfetto route requests `android.surfaceflinger.frametimeline` under `allowSystemTrace:true`; fixed expected/actual frame exports retain layer, UPID and producer jank/present fields. Requires supported Android/trace processor; missing tables are explicit gaps. |
| Physical iOS app and render-server CPU | [Activity Monitor native trace](diagnostic-native-traces.md#app-and-render-server-cpu-on-physical-ios) records a shared window. The `timeline` view returns per-process CPU percent for the app PID and `backboardd`. |
| Main/render thread work, layout, paint, image decoding | Existing native trace slices, Chrome complete trace events and Flutter timeline spans preserve source names, threads and durations. Names alone do not prove causality; overlapping inclusive work is not additive. |
| Flutter UI/build/raster | Existing Dart source and `Flutter.Frame` observations; build, raster, elapsed and vsync overhead keep separate definitions. Debug timings are not release acceptance; native/GPU display time is not inferred. |
| React / React Native | Existing browser React inspection or explicit RN probe/commit observations and Hermes CPU samples; web component ownership is separate from native views. Production/profiling hook availability varies. |
| Compose, SwiftUI, UIKit, Views | Existing platform UI/accessibility inspection and native trace evidence; explicitly exported component/view trees and named stage observations can use `ui-performance-v1`. Native Compose/SwiftUI runtime component-inspector protocols are not registered. |
| Cold/warm/hot startup | Import explicit launch, process/runtime/bundle, display, interaction and content milestones with state evidence. No automatic force-stop, launch, Activity recreation, cache clearing or fully-drawn hook installation. |
| Metal/Vulkan/OpenGL/shaders/resources | Import explicit GPU measurements with tool/device/restriction provenance and use native CPU/raster spans plus resource counters separately. GPU capture automation, per-draw decoding and physical-device restrictions are unverified; no GPU duration is synthesized. |
| Input to response/display | Import observed action milestones and screenshot/video reference labels; original QA/action/media owners retain their capture contracts. Imported links are unverified producer assertions and do not synchronize video with native clocks. |
| App/bundle/assets | `package-size-v1` flat disjoint inventory with explicit compressed/uncompressed/installed/download/section basis; existing `flutter-app-size-v1` remains its separate inclusive tree model. No archive expansion or package execution. |

See [native tracing](diagnostic-native-traces.md), [Android state](diagnostic-android-state.md),
[Flutter/Dart](diagnostic-dart.md), [RN/Expo](diagnostic-react-native.md),
[browser](browser.md) and [playback](playback.md) for actual collection controls.
Native UI/accessibility trees, framework component trees and screenshot pixels
answer different questions. An accessibility tree does not expose all native
views or a complete Compose/SwiftUI/React component tree.

## Import and query

Use a returned investigation reference from `diagnostics open`:

```sh
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id performance --source '{"kind":"import","path":"/absolute/private/acme-performance.json","format":"ui-performance-v1"}'
testrun diagnostics analyze diag-RETURNED_REFERENCE collector-RETURNED_REFERENCE --submission-id performance-analysis
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"view":"performance","performance":{"operation":"startup"}}'
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"view":"performance","performance":{"operation":"frames"}}' --limit 20
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"view":"performance","performance":{"operation":"accessibility","minTargetSize":44}}'
```

MCP uses `diagnostics_query` with those filter fields as ordinary tool arguments.
The `performance` object is strict and requires `operation`: `summary`, `startup`,
`frames`, `stages`, `interaction`, `trees`, `accessibility`, `gpu` or `sizes`.
It uses its own filters and shared cursor/limit (1–100), without ordinary
kind/clock/time/correlation/native-thread filters. Use ordinary row/timeline
queries when those filters are needed. `minTargetSize` is only accepted for
accessibility and must be greater than 0 and at most 1024 producer logical units.
It is a caller-selected candidate threshold, not an OS/WCAG rule.

Results include original row source pointers, collector/digest, capture phase,
source loss and unsupported parts. Retained queries work after the import file
is deleted. Follow `nextCursor` with identical filters to page rows. Measurement
groups have a 16-item preview and a `measurementGroups_list` route using
`collection: "measurement_groups"`; that route uses its own cursor with the
same limit. Group count/min/p50/p95/max keep
exact metric definitions, units, process/isolate/thread/layer and startup state
separate; quantiles use nearest rank (lower median for p50). They are recorded
samples, not population estimates or significance tests. Spans are never summed.
Android expected/actual rows remain separate; equal tokens alone never join
layers or reused processes. No universal 16 ms budget, inferred FPS, dropped-frame
count or CPU-to-GPU/display attribution is supplied.

Accessibility results flag unnamed visible interactive accessibility nodes and,
when requested, bounds below the declared threshold. View/component nodes are
excluded. Missing bounds cannot pass or fail a size check. Findings need platform
semantic review: no contrast, focus, reading-order, overlap, screen-reader or
accessibility-conformance result is established. Existing screenshots/video can
support a human/agent comparison but do not supply those missing semantics.

## UI performance exchange, version 1

This is a strict interchange for app/tool exports, not a raw native trace decoder.
For example:

```json
{
  "schemaVersion": 1,
  "runtime": "android",
  "buildMode": "profile",
  "instrumentation": "Acme explicit launch markers",
  "clocks": [{"id": "app", "unit": "milliseconds"}],
  "records": [{
    "type": "startup", "runId": "launch1", "state": "cold",
    "stateEvidence": "producer witnessed a new process",
    "milestones": [
      {"name": "launch_requested", "clock": "app", "at": 10, "uncertainty": 1, "definition": "launch intent observed"},
      {"name": "first_display", "clock": "app", "at": 210, "uncertainty": 1, "definition": "producer initial-display report"}
    ]
  }]
}
```

Runtime is `android`, `apple`, `flutter`, `react_native`, `browser`, `native` or
`other`; buildMode is `debug`, `profile`, `release` or `unknown`. Declare source
clocks in seconds/milliseconds/microseconds/nanoseconds without synchronization
anchors. Ordinary analysis metadata can describe alignment; this view still
refuses cross-clock interval arithmetic. All numeric times/uncertainties/metrics
are nonnegative, finite and at most 2^53−1. Converted duration metrics use
microseconds and the same ceiling. Missing observations remain missing.

| Record type | Required fields; optional fields |
| --- | --- |
| `startup` | `runId`, state (`cold`/`warm`/`hot`/`unknown`), `stateEvidence`, `milestones`. Each point has `name`, `clock`, `at`, `uncertainty`, `definition`. Allowed order: launch_requested, process_started, runtime_ready, bundle_ready, first_display, interactive, content_ready; any nonempty subset in that order. |
| `frame` | `id`, `clock`, `start`, `end`, `thread`, `definition`; optional absolute same-clock `deadline`, `presented`, boolean `dropped`. End must follow start, deadline cannot precede start, presentation cannot precede end. |
| `stage` | `name`, `clock`, `start`, `end`, `thread`, `definition`; optional `actionId`. Explicit inclusive stage interval (layout, paint, image decode, shader compilation, etc.), without name-based causal classification. |
| `interaction` | `actionId`, `points`, `evidenceRefs` (up to 16 labels). Same point fields as startup; nonempty subset in order input_dispatched, app_received, app_responded, frame_presented. Media/evidence labels are not resolved or digest-verified. |
| `tree` | `id`, `treeKind` (`view`/`accessibility`/`component`), `framework`, `nodes`. Node: unique `id`, `role`, `interactive`, `hidden`; optional `parent`, `name`, `bounds:[x,y,width,height]` in producer logical units. Parents must exist and the tree must be acyclic. Multiple roots are allowed. |
| `gpu` | `name`, `value`, `unit`, `definition`, `tool`, `device`, `captureRestrictions`. Units: nanoseconds/microseconds/milliseconds/bytes/count/percent; percent cannot exceed 100. Imported GPU observations never advertise validated capture automation. |

Startup run, interaction action, frame and tree IDs must be unique within their
record type. Consecutive observed same-clock milestones produce separate interval
rows with an uncertainty range: duration ± sum of endpoint uncertainty (lower
bound clamped to zero). Intermediate milestones are not invented. Different
clocks produce an unavailable interval. Cold/warm/hot state requires producer
evidence, but parsing does not independently establish that state. Initial
display, interactive readiness and useful content remain separately defined.

## Package inventory and comparison

`package-size-v1` requires schemaVersion 1, `platform`, `architecture`, `buildMode`,
`buildId`, `source`, `basis` and `entries`. Basis is `archive_compressed`,
`archive_uncompressed`, `installed`, `download` or `section`. Each entry has a
unique relative `path`, integer `bytes` and `category`. Entries are flat disjoint
items; do not include a parent directory's recursive total beside its children.
Paths are labels and never opened; absolute/traversal/backslash paths are refused.
No untrusted archive/tool is executed. Byte values and total are at most 2^53−1.

`diagnostics compare` adds `packageSizeObservations`: both analyses must be
package inventories with matching declared target/app, platform, architecture,
known build mode, basis and source tool. Build IDs may differ. Changed paths have
before/after byte observations and a delta only where both entries exist. A missing
path is absent from that inventory, not zero package bytes. Up to 100 differences
are returned with count/truncation; query `sizes` for paginated complete entries.
Inventory coverage may be partial. Compare measured startup milestones separately;
a smaller asset/package does not prove faster launch, less decoding or less GPU use.

Both imports share the 8 MiB raw/expanded, 64-level/300000-token JSON, 20000-row,
64 KiB row, 16 MiB index and isolated-worker deadline limits. UI imports allow
4096 records, 4096 nodes per tree and at most 32 clocks; size inventories allow
10000 entries. Query pages have a 256 KiB ceiling: reduce limit if refused.
No live host/device/build/framework/GPU export matrix or target overhead is
certified by fixture parsing.
