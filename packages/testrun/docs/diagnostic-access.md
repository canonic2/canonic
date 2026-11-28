# Diagnostic access, privacy and preparation

Inspect a diagnostic operation's effects before selecting a source:

```sh
testrun diagnostics capabilities
```

This reads built-in metadata without starting a manager, worker, device session
or provider connection. It lists registered operations, their effects, source
scope, cost classes, retention and dependencies. MCP tools also carry the same
contract in `_meta["testrun/access"]`. Missing source selection remains `unknown`;
registered support does not establish that a particular file, operation or app
is accessible. Android, Apple, React Native and Dart collection are registered with unknown
selected-target readiness. Registered native, framework, service and remote
routes have independent prerequisites; inspect their source entries and the
outcome-based `workflows` routing rather than inferring support from a platform name.
The [`android_state` source](diagnostic-android-state.md) selects resource and
device-context groups, reuses network readback, and routes media/audio to their
existing tools without automatic helper preparation. Its overhead is unmeasured.
The [`ios` source](diagnostic-apple.md) observes a selected simulator or phone
through separate routes, including explicitly selected USB/network services and
exported report directories. Optional Apple symbolication materializes retained
artifacts privately and runs bounded UUID/architecture verification and atos.
It does not install tools, prepare developer support or generate sysdiagnose.
The [`browser_operation` source](diagnostic-browser.md) copies one exact settled
browser result after verifying its original request fingerprint. It performs no
native dispatch, cancellation or artifact-path read; referenced files/models
retain their browser owner. Shared discovery includes each existing engine
contract without preparing dependencies or opening a browser.
The [`react_native` source](diagnostic-react-native.md) performs selected loopback
discovery and optional inspector attachment. Explicit options enable network/
debugger observation, fixed probe execution, runtime-global Hermes profiling and
legacy debugger replacement. These can perturb execution; app overhead remains
unmeasured and native device attribution remains unverified.
The [`dart` source](diagnostic-dart.md) reads a private authenticated VM URI file,
attaches to one selected VM/isolate and subscribes to socket-owned events.
Optional object/retaining queries and owned inspector groups can perturb target
execution/liveness; native association remains unverified. Whole-VM output,
timeline and process memory are explicit and can include other isolates. The
collector changes no global profiler/logging settings and never forces GC.

## Effects and preparation

| Operation | Observes or changes |
| --- | --- |
| `capabilities` | Built-in effects and dependency metadata |
| `prepare builtin-parser` | Starts one bounded parser self-check |
| `open` | Creates private investigation metadata |
| `start` | Reads an explicitly selected source and writes bounded private evidence; native clients can start the host ADB server; RN options can attach, run a fixed probe, control Hermes profilers and replace a legacy debugger |
| `status`, `analysis-status` | Reads private metadata; a live owner can reconcile its own retained state |
| `read`, `query`, `compare` | Explicit private content, including application secrets where present |
| `analyze` | Reads retained evidence, starts an owned bounded parser and writes a private index; explicit Apple symbolication also copies retained symbols privately and runs bounded symbol tools |
| `project` | Reads retained evidence and omits free text and unselected scope |
| `export` | Writes an explicit numeric projection, minimal/private investigation JSON/Markdown report or selected unredacted native artifact to a caller-owned file |
| `stop`, `close`, `analysis-cancel` | Stops owned work, preserving evidence and borrowed source commands |
| `remove` | Deletes the closed, settled investigation's owned data; preserves imports and external reports |

RN attachment and fixed probe execution require the selected runtime source and
its explicit options. Registered sources do not pause/resume apps, force GC,
install helpers, change device/app settings or transmit evidence remotely.
Operation effect booleans describe possible effects across source selections;
the named optional effects and source contract state when they apply.
`diagnostics_start` can transmit configured provider query selectors and the
selected owner's authorization to that provider. This is distinct from uploading
captured evidence. Import/native reads do not inherit an automatic remote request;
inspect the exact selected source before collection.
Preparation never enables rooting, changes signing, bypasses pinning, rebuilds an
app or switches credentials as recovery. Native adapters must declare their own
effects and prerequisites before those actions become available. Existing task
authority and host permission policy still apply; this discovery step grants no
additional permission and adds no mandatory approval sequence.

The parser is bundled in the same Testrun executable. Optionally verify its
versioned round-trip contract:

```sh
testrun diagnostics prepare builtin-parser
testrun diagnostics prepare builtin-parser --version testrun-diagnostics/1
```

The check sends a fixed fixture through the bounded worker's stdin and verifies
its decoder identity and result. It does not download, install, run project code
or start a manager. It is an execution/compatibility check of the bundled
executable, not a claim of independent publisher-signature verification. The
worker's process arguments contain only its fixed entry-point name. Preparation
is optional; `analyze` does not require a prior check. Unsupported components or
versions refuse before starting a worker. No provider-password configuration is
accepted; future remote/inspector adapters must use their existing credential
owner and opaque references.

## Scoped redacted projections

```sh
testrun diagnostics project diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --limit 100
```

The default `scope: "app"` requires an investigation `binding.appId`. A row must
declare the matching `data.appId`; when a process generation is bound it must
also declare that exact `data.processGeneration`. Relevant system context must
declare `systemContext: true`, matching `relatedAppId` and, when bound, matching
`relatedProcessGeneration`. Other apps, reused PIDs, unmatched generations and
unknown scope are excluded, with an `excludedRows` count. PID equality alone
cannot establish a match. These are caller/producer assertions, not OS-verified
identity or evidence that the source itself was app-isolated.

For an explicitly selected broad artifact, select `scope: "all"` deliberately:

```sh
testrun diagnostics project diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"scope":"all","kind":"metric"}' --limit 100
```

The versioned `minimal-v1` projection returns record ordinals, kinds, numeric
timestamps/values, known units, anonymous clock indexes and opaque source
references/digests. It omits names, free text, bodies, headers, URLs, paths,
source-pointer keys, correlation IDs, clock names, symbol labels and measurement
definitions. Unknown units become null. Rows are in index order; a page has 50
rows by default and at most 100. To continue, repeat the command with the
returned `nextCursor` as `--cursor` and the same filters, until it is null. A
cursor from other filters or from a rebuilt index is refused with
`invalid_cursor`; start again without one. The MCP tool `diagnostics_project`
takes the same `cursor` and `limit` arguments; while more rows remain, its
`nextOperation` gives the exact arguments for the next page. Projection does not modify raw
evidence or the parsed index.

Numeric/timing data can remain sensitive. This projection is not a promise that
arbitrary input is secret-free, a complete diagnostic interpretation or a
shareable report without review. It always reports `observationComplete: false`;
zero projected observations cannot establish a healthy app. Use the explicit
private `read`/`query` operations when the omitted content is needed for diagnosis.

Ordinary session/analysis metadata omits URL-shaped endpoints and named
credential assignments, including authenticated inspector URLs. Other free text
is not an arbitrary-secret detector. Raw reads and full queries intentionally
retain their private content. Do not use descriptive metadata as a credential
store; no sanitization changes the retained source bytes.

## Export, retention and deletion

```sh
testrun diagnostics export diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --path /absolute/path/to/acme-report.json --filters '{"scope":"all"}' --limit 100
```

Export writes one `minimal-v1` projection page, at most 256 KiB, to a new JSON
file in an existing directory outside the current private runtime namespace.
It performs no upload. New reports are private files (mode 0600 on Unix).
Existing identical bytes return `alreadyPresent: true`; different content,
symlinks and special files refuse. Repeating the same immutable page and path
after a lost reply does not overwrite another file. Existing identical files
keep their permissions. To export another page, repeat the command with the returned
`nextCursor` as `--cursor` (MCP `cursor`), the same filters and a new path.

[Native trace artifacts](diagnostic-native-traces.md) have an explicit
`format:"native_artifact"`, `scope:"all"` export, without row filters. This writes
original **unredacted private** bytes, at most 4 MiB, with exact digest checks;
xctrace bundles are tar archives. Generic raw log/heap/media export is not
registered. An export cannot enable a stronger collector or recollect evidence.

Private investigations retain up to 16 collectors and 16 analyses each, with
256 investigations per workspace; deletion is explicit, without automatic expiry.
Closing stops owned work and prevents new collectors. `remove` waits for owned
work to finish and deletes only known investigation files. Original imports,
borrowed commands and caller-owned report files survive removal.

## Cost and overhead

Discovery starts no collector or parser. Imports and operation-output readers
are `lightweight_host_io`: bounded reads, copies, durable writes and hashing.
Analysis is `deep_host_analysis`: bounded decompression, parsing, model building
and index publication in a separate process. Neither category configures app
instrumentation or a sampling rate; imported profile sampling is producer-defined.
See [format bounds](diagnostic-formats.md) before selecting deep analysis.

Settled collectors expose `cost.runWallMs` and `sourceReadCalls`. Successful
analyses expose `cost.runWallMs`, `rawBytes` and `indexBytes`. Wall time uses a
monotonic clock, includes host waits/I/O/finalization and excludes admission and
the final receipt flush. It is not app CPU time, peak memory or native capture
overhead. Older saved records and unrecorded outcomes have `cost: null`, not zero.

Measure the current host workflow on an owned import fixture:

```sh
cd packages/testrun
TESTRUN_DIAGNOSTIC_REPORT=/absolute/path/to/new-report.json cargo test --locked --test diagnostic_analysis diagnostic_access_overhead_measurement -- --ignored --test-threads=1
```

This selects three repetitions each of disabled discovery, lightweight
collection and deep analysis using the same 256-event file. It starts only an
isolated fixture manager and owned parser processes, then cleans up. It refuses
an existing report path. CLI measurements include startup, IPC and polling;
internal run measurements and retained-file growth are recorded separately.
The report states its observed host scope and limits. Native disabled/lightweight/deep capture overhead
must be measured by each later adapter on its selected app/target; those costs
are currently `not_measured`. The [Android source](diagnostic-android.md) declares
its sampling cadence, child deadlines, private report reads and target limits.
