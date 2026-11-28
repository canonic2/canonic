# Memory, allocation and retention investigations

Start with `diagnostics capabilities`. Its `memory` catalog identifies the
runtime routes, effects, limits and unsupported operations. Heap ownership,
allocation sampling, process RSS/PSS and native resources are different models;
Testrun preserves those differences rather than reporting one memory number.

## Select the runtime evidence

| Runtime/source | Route and interpretation |
| --- | --- |
| Chrome/V8 | Existing `heap_capture`, `heap_load`, `heap_query`, `heap_compare` and `heap_release` browser requests. Native DevTools owns object/class/retainer/path/dominator semantics, capture epochs and model lifetime. Retain a settled query through `browser_operation` for the shared investigation. |
| Hermes / React Native | Existing `react_native` source, `mode:"heap_sampling"`, with explicit profiler-control opt-in. Results are sampled allocation estimates with stacks, not live ownership or V8 snapshot semantics. |
| Dart / Flutter | Existing `dart` `memory`/`inspect` source reads class statistics, bounded live instances and retaining/inbound paths. IDs expire; reads can cover the shared isolate-group heap and can perturb liveness. Full binary Dart heap/dominator parsing is unavailable. |
| JVM / Android ART | Import a regular bounded HPROF as `format:"java-hprof"`. The isolated parser reads supported Java/Android records and computes a defined strong-reference model. No app, ADB, JVM, Shark process or heap-dump command runs during analysis. |
| Apple native | `native_trace` with xctrace `Allocations` or `Leaks`, or import a `.trace` bundle. Separate recording/inspection opt-ins apply to capture. Recognized advertised allocation/VM/leak tables retain producer fields and available allocation stacks. Xcode's GUI memory graph and `.memgraph` binary format are not decoded or acquired. |
| App-owned native/runtime graph | Import `memory-graph-v1` with explicit producer runtime, roots, reference strengths and byte definition. This is a producer-declared graph, not an automatically validated native heap or ARC/GC model. |
| Native resources | Existing Android state/process counters plus app-owned `memory-series-v1` for views, textures, buffers, file handles, sockets, subscriptions, timers, threads and child processes where the source measures them. Missing counters remain unavailable. |

Browser native queries use their existing typed page/developer/snapshot
references; see the [browser guide](browser.md). Hermes and Dart workflows are
in [RN/Expo](diagnostic-react-native.md) and [Flutter/Dart](diagnostic-dart.md).
Their existing cleanup and generation rules remain authoritative.

## Import and query a heap

```sh
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id heap --source '{"kind":"import","path":"/absolute/private/acme.hprof","format":"java-hprof"}' --limits '{"maxBytes":8388608}'
testrun diagnostics analyze diag-RETURNED_REFERENCE collector-RETURNED_REFERENCE --submission-id heap-analysis
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"view":"memory","memory":{"operation":"classes"}}'
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"view":"memory","memory":{"operation":"objects","className":"android.view.View"}}'
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"view":"memory","memory":{"operation":"paths","objectId":"0xRETURNED_ID","maxDepth":32}}'
```

Use returned IDs exactly. The example ID is a placeholder, never an object
selection fallback. Object IDs belong to the decoded snapshot digest; equal
addresses/IDs in another capture are not matched. Original imports can be
deleted after collection without affecting retained analysis/query replay.

The memory view has these operations, using shared `cursor` and `limit` (1–100):

| Operation | Result |
| --- | --- |
| `summary` | Runtime/format, snapshot digest, size definition, object/edge counts and model-completeness gaps |
| `classes` | Recorded object counts and defined self-byte sums per exact class name; retained sizes are not added across objects |
| `objects` | Object rows, optionally filtered by exact `className` |
| `object` | Exact `objectId`, attributes, root observations, allocation-stack references and modeled sizes |
| `edges` | Paginated outgoing references for exact `objectId` |
| `retainers` | Incoming references with explicit strong/weak/unknown strength |
| `paths` | One shortest recorded strong-reference path from supplied roots, bounded by `maxDepth` 1–64 |
| `dominators` | Objects with a computed immediate dominator and modeled retained-byte weight, where the supplied graph supports it |
| `series` | Observed resource quantities after declared release/cleanup, with baseline differences and probe references |

Object/list responses preview at most 16 edges and report `edgeCount` and
`edgesTruncated`. Use `edges` to read the rest; all references remain in the
analysis model. Normal row queries also retain the underlying object/edge chunks.
Memory view uses its own operation/class/object filters; it does not combine
ordinary kind/time/native-thread filters.

## HPROF meanings and limits

The reader admits Java profile 1.0.1/1.0.2/1.0.3, 4- or 8-byte identifiers,
single full heaps or terminated heap segments, class/static/inherited fields,
object/primitive arrays, standard/selected Android GC roots, and available
allocation-trace records. Unknown heap subrecords, truncated input, duplicate
object IDs, invalid layouts and unsupported no-data arrays refuse analysis.
Raw evidence remains retained when analysis fails. This is a bounded HPROF
reader and graph analyzer, not Shark/LeakCanary's lifecycle or library heuristics.

Instances use the producer's `CLASS_DUMP.instance_size`; arrays use element
payload bytes; class metadata contributes zero modeled bytes. Headers, array
alignment, class-object footprint, native allocations and RSS/PSS are not supplied
by this definition. A modeled retained-byte value is the sum of those weights
dominated in the **modeled strong-reference graph**, not a guarantee of bytes
reclaimed by GC or an allocator. Weak `java.lang.ref.Reference.referent` edges
are excluded from strong reachability, including inherited reference fields.
Unknown field/class semantics or missing references disable retained-size and
dominator results. Recorded paths can still be inspected without proving absence.

For Activity/view paths, inherited `android.app.Activity.mDestroyed` and
`mFinished` fields are retained when present. A path containing `mDestroyed=false`
reports `underNotDestroyedActivity`; this is only an observation on that path,
not proof of foreground/resumed state or a leak. Views under a live Activity can
be expected ownership. A destroyed Activity on a path also needs expected-release
and timing evidence before a specific leak attribution.

Raw and expanded input each have the existing 8 MiB ceiling; gzip HPROF is
supported within both limits. Binary source offsets refer to the expanded HPROF
payload. Graphs admit 10000 objects, 50000 references and 100000 HPROF records;
edge chunks contain at most 32 entries. Dominator work has a 4000000-step budget,
plus the shared worker deadline, 20000-row/16 MiB index and 256 KiB query-page
limits. Oversized models refuse rather than silently dropping references.
Large production dumps commonly need an external analyzer; no tool is installed
or downloaded automatically. A reduced/partial exported graph must declare its
missing edges/roots instead of claiming complete retained-size semantics.

## Apple allocation and VM evidence

Use the [native trace workflow](diagnostic-native-traces.md) with exact
target/PID, `allowAttach:true` and `template:"Allocations"` plus
`allowAllocationRecording:true`, or `template:"Leaks"` plus
`allowHeapInspection:true`. Imports do not require capture effect opt-ins.
Recording/inspection can stop or perturb execution; the existing finite command,
private artifact, cancellation and cleanup bounds apply.

Only recognized `heap-allocations`, `vm-tracker` and `leaks` schemas advertised
by the trace inventory are exported. Unknown/GUI-only schemas remain unavailable.
Query ordinary `kind:"allocation"`, `"metric"` or `"retention"` rows. Only an
explicit `size-in-bytes` producer field becomes a byte measurement; unknown size
columns remain raw without guessed units. Producer allocation stacks are kept
separate from CPU stacks. A Leaks instrument record is not independently validated
leak attribution or a portable dominator graph. Every template/device/simulator
combination still needs its own live validation.

## Producer graph and repeated-action records

`memory-graph-v1` accepts a strict version-1 object with `runtime` set to
`producer_v8`, `producer_hermes`, `producer_dart` or `producer_native`, a
`sizeDefinition`, `graphComplete` assertion and `objects`. Each object has a
string `id`, `className`, `kind`, `selfBytes`, optional `roots`, `edges`
(`to`, `name`, `strength`: strong/weak/unknown), scalar `attributes` and bounded
`allocationStack`. This is an interchange format for explicit app/tool exports,
not an additional raw runtime decoder. Imported derived dominators/retained
values and source pointers are replaced by owned computation/witnesses.

`memory-series-v1` has `schemaVersion:1`, explicit `runtime`,
`processGeneration`, `buildId`, optional `probeRefs` and ordered `samples`.
Each probe names a finalized local `collectorId` and exact `sha256`; public
analysis verifies both ownership and retained bytes before running the parser.
A sample has `phase`, `iteration`, `elapsedMicros`, optional `actionId` and
`probeIds`, and `counters` (`resource`, integer `value`, `unit`, `source`).
Use optional `warmup`, one `baseline` at iteration 0, then `after_action` and
matching `release` at consecutive iterations, with optional `cleanup` afterward.
Action/release IDs must agree. An unfinished last action remains an explicit
missing-release observation. Counters have explicit `count`/`bytes` units;
heap/RSS/external measurements use bytes. Up to 512 samples, 32 counters per
sample and 32 retained probe references are admitted.

For example, keep a baseline descriptor count, observe it after opening a screen,
record the app's expected close/release point, then observe cleanup and attach
the retained OS/probe/heap collectors. Query `memory.operation:"series"` to see
release/cleanup differences against the same resource/unit/source baseline.
Changing the source is not silently compared. Warmup, release and cleanup are
producer observations: this format does not run actions, dispose app resources,
force GC, send pressure/trim callbacks or reset profilers.

`diagnostics_compare.memoryClassObservations` provides class count/defined-byte
differences only for matching format/runtime/size definitions, complete modeled
graphs and equal declared target/app/build/process birth/sampling. It never matches
snapshot object IDs or sums retained bytes across owners. It does not establish
steady-state behavior, cache invalidity, GC/allocator causality or a leak.

A specific leak attribution needs a supported reference/ownership path and
evidence that the owner should have released it at the observed lifecycle point.
Allocation growth, a successful pressure callback, texture/handle counts or
persistent RSS alone cannot supply that proof.
