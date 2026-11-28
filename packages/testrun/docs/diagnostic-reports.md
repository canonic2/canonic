# Compose evidence and save an investigation report

Use the existing `diagnostics query` and `diagnostics export` commands to inspect
several retained sources and save JSON or Markdown. MCP uses `diagnostics_query`
and `diagnostics_export` with the same fields, identities, errors and privacy.
No provider/device reads, recollection, viewer launch or upload occur here.
See [symptom workflows](diagnostic-workflows.md) to choose a collector first.

All selected analyses must be complete and belong to the same investigation and
workspace. Their retained raw/index hashes are verified through the existing
reader. Select the primary `analysisId` normally, and at most three *additional*
distinct IDs in `analysisIds`; do not repeat the primary ID. The original source
file can be deleted after finalized retention; queries use the retained copy.

## Find observations across sources

Replace the illustrative IDs with returned investigation/analysis references:

```sh
testrun diagnostics query INVESTIGATION PRIMARY_ANALYSIS --filters '{"view":"investigation","investigation":{"analysisIds":["ADDITIONAL_ANALYSIS"],"operation":"summary"}}'
testrun diagnostics query INVESTIGATION PRIMARY_ANALYSIS --filters '{"view":"investigation","investigation":{"analysisIds":["ADDITIONAL_ANALYSIS"],"operation":"failures"}}' --limit 20
```

`summary` returns per-source formats, available signal kinds/counts, scope,
clocks, loss and bounded source gaps. Private summaries include up to eight
distinct root producer build/image/architecture/service/version observations and
ordinals; nested native identities need raw/domain queries. These observations
and caller build notes do not authenticate deployment identity.
Other operations return native rows:

| Operation | Selected observations |
| --- | --- |
| `timeline` | Timestamped rows ordered within each analysis/native clock |
| `failures` | Recorded failure rows |
| `stacks` | Recorded stack rows; specialized call trees remain single-analysis queries |
| `resources` | Metric and process rows |
| `frames` | Frame rows |
| `retention` | Retention and allocation rows; specialized heap traversal stays in `view:"memory"` |
| `requests` | Recorded spans, including service spans; protocol-specific details stay in `view:"network_data"` |

Composed rows carry `evidence` with analysis ID, collector ID, raw/index hashes,
ordinal and source pointer. `clockScope` keeps identically named clocks separate
across analyses. Timeline display does not imply cross-source clock agreement,
causal order or a globally synchronized trace. Source order is not a causal rank.
Use specialized queries for domain-specific calculations and identity checks.

Use `cursor`/`nextCursor` with the same view/options and `limit` 1–100. Pages
end early at 256 KiB of serialized items. A larger single row is returned alone
if it fits the transport; otherwise use the named raw reader.
Summary gaps show at most eight per source, with a
count/truncation marker; individual queries retain the full supported gaps.
Composition reads up to four existing indices (each at most 16 MiB), without
building or persisting a combined index. Narrow source selections when needed.

## Build a report with witnessed facts

```sh
testrun diagnostics query INVESTIGATION PRIMARY_ANALYSIS --filters '{"view":"report","report":{"privacy":"private","analysisIds":["CORRECTED_ANALYSIS"],"exactBuild":{"id":"acme-build-2","architecture":"arm64","mode":"release"},"reproduction":["Open checkout and submit the fixed fixture"],"facts":[{"statement":"The baseline recorded a failure","witnesses":[{"analysisId":"PRIMARY_ANALYSIS","ordinal":0}]}],"hypotheses":[{"statement":"The failure may originate at the service boundary"}],"correction":"Applied the bounded fixture correction","comparison":{"baselineAnalysisId":"PRIMARY_ANALYSIS","correctedAnalysisId":"CORRECTED_ANALYSIS"},"unresolved":["Provider sampling coverage is unknown"]}}'
```

The report includes the retained question and scope, declared exact build,
reproduction, observed row counts/witnesses, facts, hypotheses, correction,
comparison and unresolved limits. Every fact needs 1–8 existing witnesses in the
selected sources; hypotheses may have none. Testrun verifies references and
digests, not the truth of caller prose or imported build claims. Each statement
keeps its attribution. The comparison uses existing compatibility rules; a
different build, clock, sampling or incomplete observation can prevent a numeric
comparison. An incompatible result is useful evidence, not a successful correction.

Options default to empty sections. `privacy` defaults to `minimal`; it omits the
question, binding, native names/paths/clock labels, notes, measurement details and
exact build identity. Counts, digests, formats and witness ordinals remain and can
still be sensitive. Build ID can appear only as its SHA-256 in this projection.
Explicit `privacy:"private"` includes the text/identity sections and comparison
details; these can contain customer data or credentials supplied in notes. Neither
mode copies raw row data, bodies, heaps or attachment contents into the report.

Each prose section accepts at most 16 entries, each statement at most 1,024
bytes without control characters, and at most eight witnesses per statement.
`exactBuild` requires `id`, `architecture`, and `mode` (`debug`, `profile`, `release`
or `unknown`). `comparison` must name two distinct selected analyses. Report
queries do not accept pagination or ordinary row filters. The query response caps
at 256 KiB; reduce notes/source selections when it exceeds the budget.

## Save JSON or Markdown explicitly

```sh
testrun diagnostics export INVESTIGATION PRIMARY_ANALYSIS --path /absolute/acme-report.json --filters '{"scope":"all","format":"investigation-json","report":{}}'
testrun diagnostics export INVESTIGATION PRIMARY_ANALYSIS --path /absolute/acme-report.md --filters '{"scope":"all","format":"investigation-markdown","report":{"privacy":"private","analysisIds":["CORRECTED_ANALYSIS"]}}'
```

Supply the same report options/notes when exporting a previously queried report;
the report is deterministically derived, not an editable Testrun task record.
Report formats require explicit `scope:"all"`, `report` options (`{}` for
defaults), and no row filters/pagination. That scope means selected source
observations; it does not authorize a wider provider or device read. Existing
`minimal-v1` numeric projection and `native_artifact` exports keep their own scope
and bounds. Native artifact export preserves original unredacted bytes for human
tools; it never launches a viewer.

The destination must be an absolute file in an existing directory outside private
runtime storage. Files are published privately, with a 512 KiB report limit, no
overwrite of different bytes, and a receipt containing hash, size and privacy.
Repeating identical bytes returns `alreadyPresent:true`; changed evidence/notes
need a new destination. No evidence is uploaded. External reports survive
investigation removal; private report files retain their sensitive contents.
Markdown uses fenced JSON sections with escaped fence/HTML characters, preserving
structured provenance and safe boundaries between caller prose and the document.

If an analysis is not ready, inspect `diagnostics_analysis_status` or cancel its
owned parser and wait for settlement. A lost analysis reply reuses the original
submission ID/inputs. An unknown witness needs a valid selected analysis/ordinal;
it never silently substitutes another row. Hash mismatches refuse with
`EVIDENCE_CHANGED`; preserve evidence and investigate its storage owner. Missing
builds/clocks or source gaps remain unresolved instead of being treated as health
or proof of cause.

Reports/composed queries are bounded retained reads, with no separate report job
to cancel. Capture and parser cancellation keep their existing owners. After a
lost export reply, repeat the same path/options for identical-byte recovery;
changing the report requires a new destination.
