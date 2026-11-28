# Choose an investigation by symptom

Start with `testrun diagnostics capabilities` (MCP `diagnostics_capabilities`).
Its `workflows` groups supported operations by outcome; `sources` declares actual
prerequisites, scope, effects and limits. Static registration is not target
readiness. Collection never grants additional access or silently changes tools.
Use `docs_search` with your symptom, then `docs_read` on the returned guide path.
These guides ship inside Testrun; no project-specific skill is required.

## Select the smallest useful observation

| Symptom / question | Start with | Inspect retained evidence | Verify the same condition |
| --- | --- | --- | --- |
| Crash or unexpected exit | Selected Android/Apple logs or explicit crash report; [Android](diagnostic-android.md), [Apple](diagnostic-apple.md) | `kind:"failure"`, exact build/image and recorded frames; unmatched symbols stay unresolved | Repeat the triggering action on the corrected exact build; compare matching failure/assertion witnesses |
| Hang or expensive CPU path | Existing native trace or explicit runtime profile; [native traces](diagnostic-native-traces.md), [services](diagnostic-services.md) | `view:"call_tree"`/`"flamegraph"`; sampled stacks are not wall-time or wait proof | Repeat a matched workload, keeping sample units, target birth and loss visible |
| Memory grows or object remains retained | Selected retained heap/graph or resource series; [memory](diagnostic-memory.md) | `view:"memory"` retaining paths/dominators/series; heap-local IDs cannot identify objects in another heap | Repeat release/cleanup and inspect the same resource/retention condition; growth alone is not a leak |
| Slow interaction, startup or frames | Selected native/runtime frame/startup export; [performance](diagnostic-performance.md), [Dart](diagnostic-dart.md), [RN](diagnostic-react-native.md) | `view:"performance"`; preserve stage, clock, deadline and measurement definition | Apply exact metric assertions to matched before/after attempts; do not invent percentile or display guarantees |
| Request fails or data looks stale | Selected browser/network/data evidence; [network/data](diagnostic-network-data.md) | `view:"network_data"`, exact request/trace identity and consistency scope | Repeat the same request/fixture and verify response/data assertion; no implicit SQL, proxy or credential changes |
| Background, media, power or peripheral issue | Explicit lifecycle/context observations; [scenario context](diagnostic-scenario-context.md) | `view:"scenario"`; media state differs from measured audible output | Repeat the same lifecycle schedule; preserve setter-owned restoration and hardware gaps |
| Embedded/framework boundary fails | Selected native/runtime components or existing embedded endpoint; [frameworks](diagnostic-frameworks.md) | Exact witnessed component/bridge links; framework name alone grants no transport | Repeat the boundary assertion against the corrected build and component identities |
| A deployed failure spans services | Configured scoped provider GET or explicit OTLP/field export; [observability](diagnostic-observability.md) | `view:"observability"` exact trace/span/exemplar witnesses | Retain matched incident/correction exports with declared deployment identity; sampling/retention and billing gaps remain visible |
| A test runner reports failure | Existing declared project check or explicit runner report; [testing](diagnostic-testing.md) | `view:"assertions"`; preserve assertion/automation/fixture/unavailable outcomes | Re-run the same selected check with its existing runner owner; target control must have one owner |

## Complete the supported workflow

Open an investigation with the question and actual known binding, then start the
selected source. Choose stable `submissionId` values for open/start/analyze. After
a lost reply, repeat only the same ID and exact inputs or inspect status; a new
ID can recollect or repeat work. Never replay uncertain app input.

`diagnostics_status` describes each collector's `nextOperation`. Active collectors
need status or an explicit stop request; finalized evidence can be analyzed.
`diagnostics_analysis_status` reports parser settlement, bounded gaps and the
nearest query/recovery step. A finished collector is not proof that its contents
are complete, correctly scoped or interpretable. Empty/failed or unsupported
evidence may require an explicit compatible export or corrected new submission.

Read `diagnostics_query` with a small `limit`, retaining its analysis/collector/
digest/ordinal witnesses. Preserve filters and selected analyses when following
`nextCursor`. Large bodies, heaps, attachments and native traces stay in bounded
explicit reads/exports; ordinary status does not include them. A missing row or
zero count cannot establish absence without complete, correctly scoped evidence.

Use `diagnostics_plan` for a finite reproduction plan; planning dispatches no
actions. Existing action, fault, fixture and runner owners execute the steps.
Apply a bounded correction, capture a fresh attempt, and verify the same fixed
assertion or compatible measurement with `diagnostics_compare`. Keep an
insufficient-evidence result as unavailable rather than inventing a root cause.

Use [investigation reports](diagnostic-reports.md) to compose selected sources,
record observations separately from hypotheses, and save a durable report.
Close the investigation to stop its readers and preserve evidence. Remove it
only when closed and settled; caller-owned report files remain. Native artifacts
can be exported for human tools; a viewer is optional for supported analysis.
