# Diagnostic formats and query semantics

Testrun analyzes finalized private evidence through explicitly selected decoders.
Import the file with `diagnostics start`, then use `diagnostics analyze` as shown
in [diagnostic investigations](debugging.md). Parsing never attaches to an app,
loads a local checkout, downloads symbols or recollects the source.

## Selected decoders

| Import `format` | Accepted input and interpretation |
| --- | --- |
| `testrun-events-v1` | The versioned event exchange below; explicit units, clock domains and correlation IDs |
| `chrome-trace` | A JSON event array or `traceEvents` object; `X` durations in microseconds, `M` metadata, instant/marker events and numeric `C` counters |
| `har` | HAR 1.2 `log.entries`; request durations in milliseconds and HTTP status failures; original wall timestamps and unavailable timing phases are preserved |
| `speedscope` | The official speedscope schema with sampled weights or balanced evented stacks; exclusive evented intervals retain axis units, byte weights remain bytes |
| `android-diagnostics-v1` | Newline JSON packets from bound Android log/state collectors; installed/process/log/exit/service/report/symbol and resource observations with explicit clocks, units and loss |
| `android-tombstone` | UTF-8 native tombstone text; candidate signal, unresolved native backtrace rows and available ABI/package identity |
| `android-anr` | UTF-8 ANR text/thread dump; candidate failure and thread/backtrace observations with per-process identity where available |
| `android-bugreport-text` | Explicitly supplied broad UTF-8 system report; witnessed failure/thread/frame observations with unknown app scope; no ZIP/protobuf decoder |
| `apple-diagnostics-v1` | Bounded native Apple packets; simulator/device route, installed/process/log/resource observations and explicitly selected exported reports |
| `apple-ips` | Apple IPS JSON or header/body JSON; exception/termination candidates, threads and checked image-relative frame locations; embedded Jetsam process reports keep system scope |
| `apple-crash-text` | Legacy text crash/hang report; explicit report identity and raw backtraces; symbol addresses require matching image ranges/architecture |
| `apple-jetsam` | System process/reason/page observations; names/PIDs alone do not establish app identity or historical generation |
| `apple-metrickit` | Field metric/diagnostic payloads retaining begin/end windows, encoded units/histograms and delayed-report meaning |
| `apple-sysdiagnose-text` | Explicit broad bounded UTF-8 system text; no native generation, archive expansion or automatic app association |
| `browser-operation-v1` | One settled browser result with original request/session/resource identity, native result fields/events and source-loss witnesses; native heap/trace models remain browser-owned |
| `react-native-diagnostics-v1` | Bounded RN/Expo NDJSON packets; unresolved JS/async failures, script/map references, network coverage, Hermes CPU sample counts/allocation estimates and deduplicated optional probe/React commit records; native association remains unverified |
| `dart-diagnostics-v1` | Dart VM Service NDJSON: isolate/GC/error/async events, Flutter frame timings, existing CPU/timeline data, heap/external/class snapshots and bounded live retaining witnesses; unknown native attribution and temporary object identity remain explicit |
| `flutter-app-size-v1` | Flutter n/value/children analysis tree with target type, optionally wrapped by schemaVersion/buildId/architecture/sizeAnalysis; inclusive producer subtree bytes and unresolved exact symbol references; no package or AOT binary decoding |
| `native-trace-v1` | Native source envelope with digest-checked original artifact and fixed Perfetto CSV, simpleperf report-sample, xctrace XML or macOS sample projections; explicit scheduling/sample/loss/identity and symbol gaps |
| `java-hprof` | Bounded Java/Android HPROF 1.0.1–1.0.3 with 4/8-byte IDs; class/inherited/reference graph, selected roots, arrays and optional allocation stacks; modeled sizes and private snapshot-local IDs |
| `memory-graph-v1` | Producer-declared runtime graph with explicit roots/reference strengths and byte definition; derived dominators and witnesses computed by the owned parser |
| `memory-series-v1` | Warmup/baseline/action/release/cleanup resource observations with explicit units/source, process/build declarations and finalized local probe digest references |
| `ui-performance-v1` | Explicit startup/interaction milestones, frame deadlines, stages, distinct UI trees and GPU producer observations; [performance definitions](diagnostic-performance.md) |
| `package-size-v1` | Flat disjoint producer byte inventory with explicit size basis, platform/architecture/build/source; no archive execution or inferred startup impact |
| `network-evidence-v1` | Explicit request/phase/retry/cache/cancellation, stream/message and service span/log observations with scoped request/trace/span IDs |
| `data-snapshot-v1` | Scalar local/remote exports with role/consistency/coverage, declared indexes and bounded named read plans; no database execution |
| `pcap` | Classic PCAP Ethernet/raw IPv4/IPv6 TCP/UDP header observations; private raw payload retained, without decryption or reassembly |
| `scenario-context-v1` | Explicit lifecycle/media/peripheral/energy and owned-setting ledger observations, with app/device scope, clocks, hardware and effect declarations |
| `runtime-checks-v1` | Bounded producer-normalized ASan/TSan/UBSan, StrictMode, race/lock and runtime checks with declared process/build/environment and stack roles; [report contract](diagnostic-interactive.md) |
| `sanitizer-report-text` | Bounded native ASan/TSan/UBSan text blocks; no inferred process identity, clock or symbolication |
| `reproduction-v1` | Bounded run/attempt evidence with exact declared environment, fixture/fault schedule, failures, cleanup and measurement units; [controlled comparisons](diagnostic-reproduction.md) |
| `test-results-v1` | Normalized runner/case/attempt outcomes, checks, attachment references and bounded failure-capture declarations; [test evidence](diagnostic-testing.md) |
| `framework-evidence-v1` | Inline runtime/native components with exact declared builds/generations and witnessed links; [framework evidence](diagnostic-frameworks.md) |
| `runtime-samples-v1` | Explicit typed coroutine/recomposition/managed/engine/bridge exporter snapshots; not a vendor protocol or complete census |
| `embedded-cdp-v1` | Bounded selected-page discovery and Runtime/Page/Log/optional Network events; native association remains unverified |
| `service-evidence-v1` | Exact declared host/process/runtime/workload and lifecycle owner around one supported artifact; [service diagnostics](diagnostic-services.md) |
| `host-process-v1` | Selected native process birth, parent where available and resource counters with explicit snapshot semantics |
| `box-log-v1` | Existing Box operation/service JSON log tails; rotation and offline liveness remain unknown |
| `js-cpu-profile` | Compatible Node/Bun/Deno CPU-profile tree, sampled stacks and optional producer microsecond deltas |
| `jfr-json` | Selected `jfr print --json` events, raw thread/GC/lock/allocation/I/O values and execution-sample stacks |
| `pprof` | Raw/gzip profile.proto samples with separate type/unit columns, labels, mapping and producer function identities |
| `stackprof-json` | Public StackProf JSON profile hash with exclusive frame sample counts and raw inclusive totals/edges |
| `xdebug-cachegrind` | Selected line-position Cachegrind costs, file/function compression and separate self/call event columns |
| `windows-minidump` | Selected native system/thread/module/exception metadata; no unwind, memory dereference or PDB lookup |
| `observability-evidence-v1` | Exact query scope/window, credential owner, explicit provider pages, limits and provenance; [remote observability](diagnostic-observability.md) |
| `otlp-json` | Resource spans/logs/metrics, schema scope, exact trace/span IDs and exemplar associations; signal-specific units and raw aggregation fields |
| `prometheus-json`, `loki-json`, `sentry-json`, `jaeger-json` | Selected native JSON result/event/trace shapes; explicit provider gaps and source witnesses |
| `cloudwatch-json`, `crashlytics-json`, `play-vitals-json` | Selected logs/metrics or field-event JSON exports, with continuation/aggregation uncertainty |
| `deployment-evidence-v1` | Deployment, feature/configuration, audit/resource declarations and release/image/artifact references; no checkout identity inference |
| `junit-xml` | Selected bounded JUnit XML dialect with failure/error/skip classification and testcase durations; incomplete identity/coverage remains explicit |

Chrome begin/end and other unmodeled phases remain raw events and appear in
`unsupported`; they are not synthesized into durations. Counter units are
`producer-unspecified`. HAR response codes of 400 or above are observations of
HTTP errors, without asserting their cause. HAR wall timestamps remain original
strings; this decoder does not align them to numeric time ranges. Speedscope
evented profiles require balanced ordered stacks. Producer frame names remain producer-provided;
weights are not a CPU utilization measurement.

Unregistered native trace/profile/heap/crash files, binary JFR/EventPipe and OTLP can be retained as raw
imports with descriptive format labels. This decoder set does not analyze those
formats. An unsupported selection returns `UNSUPPORTED_DIAGNOSTIC_FORMAT` in its
analysis receipt; no heuristic format detection or native viewer invocation runs.

[Native trace sources](diagnostic-native-traces.md) convert explicitly selected
recordings with trusted installed tools before retained analysis. Query `view`
supports `rows` (default), `timeline`, `call_tree`, `flamegraph`, `memory`,
`performance`, `network_data` and `scenario`. [Scenario context](diagnostic-scenario-context.md),
[network/data](diagnostic-network-data.md),
[memory](diagnostic-memory.md) and [performance](diagnostic-performance.md)
views use their own strict options. Timeline sorts
one selected clock; native weighted-stack views preserve units/thread identity
and witnesses. CPU samples never supply off-CPU time. Native artifacts can be
exported explicitly as original private bytes; the ordinary projection remains
the export default.

[Browser operation packets](diagnostic-browser.md) index bounded native result
fields/array entries and operation outcomes. Fields above 32 KiB retain raw
witnesses with explicit row gaps. Native timing/units, cursor loss and resource
references keep their original meaning; no renderer process or shared clock is
inferred. Existing heap/performance queries supply native analysis results;
retaining a receipt does not load its files or create a new raw-format decoder.

Plain UTF-8 JSON (with an optional BOM) and gzip-wrapped JSON are supported for JSON formats. Android/Apple text formats and gzip-wrapped text are also accepted. Raw and
expanded input each have an 8 MiB ceiling. Gzip decoding validates the stream and
refuses expansion beyond that ceiling. ZIP, tar and other archives are not
expanded. A 64-level nesting ceiling, 300000 structural-token budget, 20000-row
ceiling, 256-frame sampled-stack ceiling and 16 MiB index ceiling bound parsing.
Rows have a 64 KiB encoding ceiling; expanded sample frames are checked against
32 KiB before copying, and cumulative model size is checked during construction.
The worker has a 10-second wall deadline, Unix CPU limit and Linux address-space
limit of 512 MiB. Other hosts use bounded input/model allocations without an OS
resident-memory guarantee. Cancellation kills and reaps the owned worker.

## Event exchange, version 1

Android packet details and collection restrictions are in the
[Android guide](diagnostic-android.md). Packet witnesses use `line:N` (zero-based)
and field paths; report witnesses add text line numbers or a decoded-base64 path.
Raw local-time exit timestamps are preserved without assuming a timezone.
Packet identity is producer-reported when imported; parsing does not authenticate
an imported producer. Selected report labels alone do not prove a crash/ANR.
Unrecognized report text remains an explicit gap. Multi-process/broad report
scope is never inferred from a single header. `sampleCount` counts stack rows;
backtrace/thread report observations are not CPU samples.

```json
{
  "schemaVersion": 1,
  "clocks": [{ "id": "app", "unit": "milliseconds" }],
  "records": [
    {
      "kind": "metric",
      "name": "rss",
      "timestamp": 110,
      "clockDomain": "app",
      "value": 65536,
      "unit": "bytes",
      "definition": "resident memory of the selected process",
      "correlations": ["action:42", "request:abc"],
      "data": { "processGeneration": "start-a" }
    }
  ]
}
```

Each record requires `kind`, `name` and `data`. Optional numeric `timestamp`
requires `clockDomain`, whose ID must exist in `clocks`. Clock units are seconds,
milliseconds, microseconds or nanoseconds. Optional `value` requires `unit` and
`definition`; the decoder does not infer what a metric measures. Correlation IDs
are explicit producer assertions. Testrun preserves unmatched records and source
order, and does not join events by nearby timestamps or equal PIDs/object IDs.

Supported kinds are `event`, `failure`, `metric`, `stack`, `thread`, `allocation`,
`retention`, `span`, `frame`, `action`, `marker`, `process`, `screenshot`, `video`
and `audio`. `data` preserves producer details such as thread IDs, unresolved
addresses, frame boundaries, retaining paths or existing QA/media references.
This exchange does not manufacture native heap dominators, decode media or
verify externally supplied QA references. Allocation and retention rows require
their producer's definitions and heap generation for meaningful comparison.

Every returned row has a decoder-generated JSON pointer to its raw source,
collector reference and raw SHA-256. A supplied `source` field is replaced by
that actual pointer. Gzip source pointers refer to the decoded JSON; the digest
still identifies the original retained compressed bytes.

## Clocks, drift and symbols

Analysis `--metadata` can add clock alignment without modifying raw evidence:

```json
{
  "clocks": [{
    "id": "app",
    "unit": "milliseconds",
    "referenceDomain": "host",
    "maxDriftPpm": 100,
    "anchors": [{ "source": 100, "reference": 1000, "uncertainty": 2 }]
  }],
  "sampling": "all instrumented markers",
  "observationComplete": false,
  "sourceLoss": ["app event buffer may omit earlier events"]
}
```

Alignment must match an observed clock ID and its exact unit. Anchor source,
reference and uncertainty values all use that source unit. Results use seconds
in the named reference domain. The nearest anchor provides an offset;
uncertainty grows by elapsed source distance times `maxDriftPpm`. Without an
anchor and declared drift bound, `alignedTime` is null. The raw timestamp remains
unchanged. No cross-domain ordering or causality is inferred from overlapping
uncertainty intervals. Alignment and completeness metadata are caller-declared.

Optional `symbols` entries have `kind`, `evidenceId`, `sha256`, `buildId` and
`architecture`. Kinds are `source-map`, `dsym`, `elf`, `mach-o`, `pdb`, `r8`,
`proguard` and `dart`. First import the symbol artifact into the same
investigation; its reference and digest must identify settled retained bytes.
The row's `data.buildId` and `data.architecture` must both match exactly for
`reference_matched`; absent or mismatched identities stay explicit. UUIDs,
build IDs, obfuscation-map release IDs and JS/Dart update IDs belong to the
captured artifact, not the current local checkout.

Reference matching alone leaves `resolved: false`; producer frame names keep
their provenance. [Explicit Apple symbolication](diagnostic-apple.md#explicit-dsymmach-o-symbolication)
can inspect completely retained Mach-O/DWARF bytes with dwarfdump and atos, verifying
artifact-internal UUID and exact architecture before resolving known load/PC
locations. It needs `metadata.symbolicate: true`, adds bounded host tool execution
and private temporary copies, and never downloads symbols. Other symbol formats
remain reference-only. Raw addresses and unmatched/unresolved frames are retained.

## Queries and comparisons

[Android state](diagnostic-android-state.md) packets produce metric/event/thread
rows with source definitions, scope, process generations and caller-declared
action context. Frame clocks remain distinct from cycle observation timestamps.
Comparison's `observedAndroidState` returns bounded scalar differences only for
exact known matching identities and nonoverlapping boot-uptime windows; it leaves
whole-capture compatibility unchanged and makes no retention or causal claim.

`observedDartSnapshots` in comparison reports compatible before/after class
statistics only within the same collector epoch, with matching producer VM/
isolate/root/class/reset identity and ordered VM clocks. Fresh captures have
different epochs; no cross-reload object matching or leak conclusion is inferred.
See the [Dart guide](diagnostic-dart.md) for explicit whole-VM scope and read effects.

Queries filter `kind`, `clockDomain`, `from`, `to` and `correlationId`. Numeric
ranges use the selected original clock's unit and require `clockDomain`.
Numeric projections use double precision; the raw witness retains exact source
number text when integer/decimal precision matters.
Pagination uses `cursor` and `limit` (default 50, maximum 100); follow
`nextCursor` until null. Pages also have a 256 KiB projection budget. A larger
single row is returned alone if it fits the transport; otherwise use the named
raw evidence reader. Queries return source-order observations with `aggregation: none`,
measurement units/definitions, source loss, capture phase and unsupported parts.
No parsed application event count is inferred from the collector's read chunks.

Comparison checks both analyses' format/parser, declared app/build/image/target,
process and runtime generations, sampling, clocks, completeness, measurement
units/definitions and heap identity where relevant. Unknown required facts and
incompatibilities are returned, with no delta asserted. Compatible results
report observation-count differences and separate scalar measurement summaries
with sample counts, min/max and unweighted means (at most 200 groups, with an
explicit truncation flag); overlapping spans are never
summed. These summaries are observations, not statistical significance, tail
guarantees, causal findings or an assertion of application health.

A successful parse establishes a usable index. It does not prove a complete
observation window. `observationComplete` requires an explicit caller assertion,
complete collection, no declared source loss and no unsupported sections. Zero
matches always remain zero recorded observations, including when that flag is
true; they do not prove an absent defect.

Format references: [speedscope schema](https://www.speedscope.app/file-format-schema.json),
[HAR 1.2](https://github.com/ahmadnassri/har-spec/blob/master/versions/1.2.md)
and [Perfetto's Chrome JSON overview](https://perfetto.dev/docs/getting-started/other-formats).
