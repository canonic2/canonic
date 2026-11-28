# Collect and inspect diagnostic evidence

Start with [symptom workflows](diagnostic-workflows.md) to choose the smallest
useful source. Use [investigation reports](diagnostic-reports.md) to compose
retained analyses and save witnessed JSON/Markdown without a project skill.

[Background/media/device context](diagnostic-scenario-context.md) adds source-defined
lifecycle, media/peripheral, energy and setting-restoration reports plus optional
Android context reads, without automatic active probes or settings changes.

[Network and data](diagnostic-network-data.md) adds request/phase/identity queries,
bounded packet headers, indexed scalar snapshots and explicit remote read exports
through existing authenticated access owners.

[Startup, rendering and UI performance](diagnostic-performance.md) combines
existing frame/span sources with explicit startup/interaction, UI-tree, GPU and
package-size observations through the shared performance view.

[Memory and retention](diagnostic-memory.md) covers runtime-specific heap routes,
bounded HPROF/graph queries, modeled retaining paths/sizes and repeated resource
observations with verified local probe references and explicit leak limitations.

[Native traces and hangs](diagnostic-native-traces.md) add explicit Perfetto,
simpleperf, xctrace and macOS sample sources, retained timeline/weighted-stack
queries and original private native-artifact export through the same tools.

Use a retained investigation to collect related evidence without managing another
device session. Collectors copy an explicitly selected local file, observe an
existing Testrun build/project operation or collect bound Android app evidence.
See [Android crashes, ANRs and disappearance](diagnostic-android.md) for the
native source, user/UID scope and incomplete sampling semantics.
[Android resource state](diagnostic-android-state.md) adds selectable snapshots,
measurement series and observed before/after differences through the same APIs.
[Apple diagnostics](diagnostic-apple.md) adds separate simulator/physical routes,
report ingestion and explicit retained dSYM/Mach-O symbolication.
[Browser investigations](diagnostic-browser.md) retain settled browser results
and native analysis output through the same evidence/query workflow.
[React Native and Expo](diagnostic-react-native.md) add explicit inspector
discovery, bounded logs/network/script observations, Hermes sampling and optional
React commits/markers through the same investigation owner.
[Flutter and Dart](diagnostic-dart.md) add private VM Service discovery, exact
isolate generations, frame/CPU/class snapshots, bounded retaining queries and
Flutter size artifacts through the same retained tools.

Each investigation belongs to the calling workspace. Several clients in that
workspace can inspect the same investigation; another workspace cannot resolve
its references. Ordinary target selection and browser diagnostics keep their
existing interfaces.

## Open an investigation

```sh
testrun diagnostics open --submission-id acme-startup --question "Why did Acme startup fail?"
```

Keep the returned `investigationId`. Repeating the same submission ID and inputs
returns the existing record; different inputs with that ID refuse. Optional
`--binding` JSON records app/build/process/runtime and existing QA/action
references. These are caller-declared metadata, not verified installation state
or permission to attach. Process and runtime identities require a generation
alongside the PID/runtime ID so later restarts cannot silently reuse the binding.

## Copy a diagnostic file

```sh
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id crash-copy --source '{"kind":"import","path":"/absolute/path/to/acme.ips","format":"apple-ips"}'
```

Select a regular file of at most 8 MiB. The format is a descriptive label; this
collector copies bytes. Analysis is a separate explicit operation. The original
file is preserved. A changed file, special file or final symlink is refused or
reported as incomplete. Capture uses an opened file descriptor and checks size
and modification time before reporting completion.

Collection returns immediately with a retained `collectorId`. Reuse its
submission ID after a lost reply; Testrun never reopens the source on that replay.
Use a new ID only when you intend another collection.

## Observe an existing operation log

```sh
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id server-output --source '{"kind":"build_output","operationId":"build-RETURNED_REFERENCE"}'
```

The source must already exist in this workspace. Testrun observes its retained
output, including output from declared project tasks/servers. Stopping the
collector does not cancel the command, build, service or another collector.
The captured format is the existing log reader's UTF-8 projection, which may
replace malformed UTF-8; it is not a byte-identical source-log archive.

Collectors default to 30 seconds, 1 MiB and 4096 nonempty read chunks. Optional
`--limits '{"durationMs":10000,"maxBytes":65536,"maxEvents":100}'` selects bounds:
duration 1–300000 ms, bytes 1–8388608, and events 1–65536. An event here is a read
chunk, not a parsed application log event or CPU sample. Hitting a bound preserves
partial evidence and reports the reason. A log collector completes after the
source settles and all observed output has been drained.

## Inspect and finish

```sh
testrun diagnostics status diag-RETURNED_REFERENCE
testrun diagnostics read diag-RETURNED_REFERENCE collector-RETURNED_REFERENCE --limit 16384
testrun diagnostics stop diag-RETURNED_REFERENCE --collector-id collector-RETURNED_REFERENCE
testrun diagnostics close diag-RETURNED_REFERENCE
```

Status reports phases, byte/chunk counts, limits, source identity, completion and
loss reasons. It contains no captured content or import path. Explicit reads
return private bytes as `dataBase64` with nullable `nextCursor`, up to 65536 bytes
per read. Follow it with `--cursor`, keeping the same investigation and collector.
Finished output ends with null; live collectors report `running` and retain a
cursor at their current end.
Raw output can contain application secrets. Add `--json` for machine-readable
CLI responses; MCP returns the same data through structured tool results.

Stop preserves evidence; omitting `--collector-id` stops every collector in the
investigation. Close also prevents new collectors. Stop/close return the current
state: poll status until `settled` is true before claiming finalization. Successful
finalization records the retained output's SHA-256; later reads refuse changed
evidence. A finalization failure remains `cleanup_pending`; status retries owned
finalization without reading the original source again.

The manager keeps active readers alive independently of the client that started
them. Target release stops readers explicitly associated with that target.
Draining/stopping the manager stops all readers. If the manager or host dies,
retained evidence is reported partial with `owner_lost`; reconnecting or repeating
start does not resume or recollect it. Status/read work without starting a manager
or reconnecting a device. Offline stop cannot signal a lost owner and reports the
retained partial outcome. These offline inspections do not rewrite records.

To delete retained data, first close the investigation and wait for settlement:

```sh
testrun diagnostics remove diag-RETURNED_REFERENCE
```

Removal affects only the investigation's known files and refuses a directory
containing unowned files. Source files and commands remain untouched. An explicit
removal also discards its replay receipts. Retention is bounded to 256
investigations per workspace, 16 collectors per investigation and 16 active
collectors per manager. Remove closed investigations to free workspace capacity.

### Read a capture result

After [copying a file](#copy-a-diagnostic-file), inspect the collector with
`testrun diagnostics status diag-RETURNED_REFERENCE --json`. For example, the
collector for an imported Acme artifact might contain these fields:

```json
{
  "sourceKind": "import",
  "phase": "complete",
  "settled": true,
  "bytes": 2048,
  "eventUnit": "read-chunk",
  "reason": "source_complete"
}
```

`phase: "complete"` confirms that the selected bytes were retained. `bytes`
describes the capture size. Read the retained artifact with `diagnostics read`
or analyze it below to identify individual records. For `phase: "partial"`,
inspect `reason` before drawing conclusions. `settled: true` is also reported
for partial and failed captures.

## Analyze retained evidence

After collection settles, build a reusable index with a stable submission ID:

```sh
testrun diagnostics analyze diag-RETURNED_REFERENCE collector-RETURNED_REFERENCE --submission-id parse-once
testrun diagnostics analysis-status diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"kind":"failure"}' --limit 100
testrun diagnostics analysis-cancel diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE
testrun diagnostics compare diag-BASELINE analysis-BASELINE diag-CANDIDATE analysis-CANDIDATE
```

Select `testrun-events-v1`, `chrome-trace`, `har` or `speedscope` as the import's
format. See [formats and query semantics](diagnostic-formats.md) for accepted
schemas, parser bounds, timestamps, clock alignment, symbol references and
comparison requirements. Optional `analyze --metadata` takes JSON with those
declared facts. An analysis ID is scoped to its investigation and submission ID;
reuse returns its receipt without launching another parser. Changed inputs with
that submission ID refuse. Use a new submission ID for a deliberate new analysis.

Analysis runs in a bounded separate process with at most four active analyses
per manager and sixteen retained analyses per investigation. Cancellation and
manager drain stop the owned parser and preserve original evidence. A lost
owner leaves a failed receipt; reconnect does not restart parsing. Query/status
work offline and never recollect. Queries verify raw and derived digests before
reading a retained index. Closing stops collectors and analyses; removal waits
for owned work to finish and deletes the investigation's known derived files.

Queries expose private content and preserve uncorrelated observations. Empty
results and successful parsing do not establish a healthy app. Unsupported native
formats remain readable as raw evidence and return an explicit parser refusal.

### Read a failure row

The `diagnostics query` command above filters the retained analysis to failure
rows. An illustrative row from a `testrun-events-v1` capture might include:

```json
{
  "kind": "failure",
  "name": "checkout request timed out",
  "timestamp": 1274,
  "clockDomain": "app-run"
}
```

The producer recorded a timeout. Use the `app-run` clock definition to interpret
`1274` and compare it with nearby rows from that clock. Check the source loss and
coverage before inferring how often the failure occurred. Follow `nextCursor`
when the query has more pages.

## MCP operations

For effect/cost discovery, dependency self-checks, scoped numeric projections and
separate report exports, see [diagnostic access](diagnostic-access.md).

`diagnostics_open`, `diagnostics_start`, `diagnostics_status`, `diagnostics_read`,
`diagnostics_stop`, `diagnostics_close` and `diagnostics_remove` mirror the CLI.
Analysis uses `diagnostics_analyze`, `diagnostics_analysis_status`,
`diagnostics_analysis_cancel`, `diagnostics_query` and `diagnostics_compare`.
Access uses `diagnostics_capabilities`, `diagnostics_plan`, `diagnostics_prepare`,
`diagnostics_project` and `diagnostics_export`.

Use [reproduction plans and comparisons](diagnostic-reproduction.md) for bounded
presets, retained action markers, existing fault-control owners and repeated
attempt evidence across different exact builds. Planning dispatches no work.
Use [test evidence and assertions](diagnostic-testing.md) for declared runner
reports, case/attempt associations and fixed predicates over retained evidence.
Use [framework and embedded evidence](diagnostic-frameworks.md) for explicitly
selected CDP pages, runtime exports and witnessed native/runtime composition.
Use [desktop and service diagnostics](diagnostic-services.md) for selected host
process snapshots, Box-owned log tails and concrete runtime profile imports.
Use [remote observability](diagnostic-observability.md) for configured provider
GETs, retained OTLP/field exports, deployment declarations and exact trace joins.
These operations require manager feature `diagnostic_observability_v1`.
Fields use camelCase, including `submissionId`, `investigationId`, `collectorId`
and `source`. Discover registered source kinds and restrictions with
`diagnostics_capabilities`; native/framework collectors require their exact binding.
An older manager must support `diagnostic_sessions_v1` for collection and
`diagnostic_analysis_v1` for analysis, and `diagnostic_access_v1` for projection/export.
Static discovery and bundled verification run without a manager. Existing browser `diagnostics_*` requests remain nested under
`browser_send` and keep their browser-specific scope and schemas.
