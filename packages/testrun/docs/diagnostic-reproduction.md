# Reproduction and controlled comparisons

Use `diagnostics plan` to choose a bounded investigation before changing an app.
Planning reads built-in metadata only: it starts no manager, target, collector,
network request or input. Historical evidence can establish a failure without
requiring another reproduction.

```sh
testrun diagnostics plan memory_growth --platform android --options '{"targetId":"android:SELECTED","appId":"com.example.acme","userId":0,"durationMs":30000,"intervalMs":1000}'
```

Presets: `crash`, `hang`, `memory_growth`, `slow_interaction`, `startup`, `network`,
`background_media`, `soak`. Platforms: `android`, `ios`, `browser`, `import`.
The result includes a source and collector limits where possible, missing
selectors, existing fault-control owners, cleanup instructions and evidence
requirements. Read `readiness:not_probed`; a registered route does not establish
device/tool/build availability. Missing selectors produce no executable source.
For browsers select an exact owned context/page, check engine capabilities, use
the existing capture operations, then retain their settled operation results.
Imports require an explicitly selected historical file.

Start a returned source through `diagnostics_start` with its returned `limits`.
Android follow collectors sample resources repeatedly within that finite budget;
they stop on source loss, cancellation or byte/event/duration limits, with at most
256 cycles. Command time is additional to the sampling interval. Plans accept
1–300 seconds and 500–60,000 ms intervals, no interval greater than the window.
`soak` selects repeated observation, not repeated input or an unattended campaign.
Use `diagnostics stop`, then inspect settlement before removing evidence.

## Actions, faults and determinism

Open an investigation with declared action IDs. Start capture before an action,
then add a retained host marker using a stable submission ID:

```sh
testrun diagnostics open --submission-id checkout --question 'Investigate checkout' --binding '{"targetId":"android:SELECTED","appId":"com.example.acme","actionIds":["checkout"]}'
testrun diagnostics start INVESTIGATION --submission-id before-checkout --source '{"kind":"marker","name":"before checkout","actionId":"checkout","phase":"before_action"}'
```

Markers require an action ID in the investigation binding. Phases are
`capture_start`, `before_action`, `after_action`, `fault_applied`, `fault_removed`,
`capture_stop`, `cleanup`. They record host wall-clock milliseconds at admission;
they do not prove app input, completion, causality or fault restoration. Host wall
time may jump and is not automatically aligned to app/native clocks. A repeated
submission reads the retained marker rather than taking a new timestamp.
Each distinct marker uses one of the investigation's 16 collector slots; import
a combined evidence file for larger externally produced marker histories.

Perform the existing `act` batch/wait, or record the externally performed action.
Inspect uncertain outcomes before sending more input. Planning never executes or
replays an action. Select a `fault` in plan options to locate its owner:

| Fault | Existing owner and release |
| --- | --- |
| `offline` | Native `network_set` affects the selected device network and can affect other apps; browser `context_configure` offline scopes to its owned context. Use that owner's receipt/readback and cleanup contract |
| `delay`, `response` | Browser `network_route` plus `callback_resolve`; resolve held requests and remove only the installed handler |
| `permission` | Browser context permissions; use a fresh isolated context and close it afterward |
| `lifecycle` | Native app launch/terminate/restart through `act` and existing controller ownership; app state is not reversibly snapshotted |
| `pressure` | Unregistered by this workflow; no fallback shell command |

Unsupported platform/fault pairs return `availableRoute:false`. Existing
permissions and effects still apply. A fault plan is not an execution receipt.
Readbacks and fresh authority belong to the setter; imported restore labels do
not authorize a setting change. See [scenario context](diagnostic-scenario-context.md).

Browser `context_clock` controls exposed virtual-time facilities, and owned
network response fixtures can make supported requests deterministic. Close the
owned context/handlers according to their contracts. Native data, server state,
randomness and app clocks require explicit app/provider test controls; Testrun
does not replace random generators, reset databases/caches or infer production
replay from a trace. See [browser controls](browser.md) and [network evidence](diagnostic-network-data.md).

## Retain every attempt and compare builds

Import `reproduction-v1` for one bounded run. This is producer evidence, not a
runner: it never opens references, executes commands or authenticates identities.

```json
{
  "schemaVersion":1,"runId":"baseline-1","scenario":"checkout","scenarioVersion":"v1",
  "buildId":"build-a","targetId":"android:SELECTED","appId":"com.example.acme",
  "runtime":"ART","osVersion":"API36","hardware":"emulator","mode":"debug",
  "warmState":"warm","warmupAttempts":2,"sampling":"elapsed per attempt",
  "instrumentation":["markers"],"contention":"isolated fixture","fixtureId":"data-v1",
  "metric":{"name":"checkout","unit":"milliseconds","definition":"request to rendered result","direction":"lower"},
  "budget":{"maxAttempts":2,"maxDurationMs":10000,"elapsedMs":2000},
  "termination":"completed","cleanup":"verified","coverage":"complete",
  "attempts":[
    {"id":"a1","actionId":"checkout","atMs":100,"outcome":"passed","reason":"result observed","value":100,"evidenceRefs":["capture-1"]},
    {"id":"a2","actionId":"checkout","atMs":1000,"outcome":"passed","reason":"result observed","value":120,"evidenceRefs":["capture-2"]}
  ]
}
```

Each run has exact declared build, target/app, OS/runtime/hardware, mode, warm
state, warmup, sampling, instrumentation, contention, fixture and metric fields.
Build IDs may differ between baseline/candidate. Other definitions and action/
fault schedules must match. Query the retained `process` row for run context and
`metric`/`failure` rows for attempts. Attempt and action IDs are correlations.
No timestamp is inferred from run-relative `atMs`.

Attempt outcomes: `passed`, `failed`, `invalid`, `unavailable`, `cancelled`.
`value` is optional; missing measurements and unsuccessful attempts remain in
the evidence and disqualify quantitative comparison. Include failures rather
than discarding them or rerunning until green. Optional `fault` fields are
`kind`, `owner`, `scope`, `before`, `applied`, `after`, `cleanup`, `receiptRef`.
Kinds: `offline`, `delay`, `response`, `lifecycle`, `permission`, `pressure`,
`fixture`. Fault cleanup accepts `verified`, `failed`, `unknown`, `not_reversible`;
verified cleanup also requires matching before/after values for comparison.
These are declarations, not verified setter receipts.

Termination: `completed`, `cancelled`, `failure`, `budget_exhausted`,
`cleanup_failed`, `source_lost`. Run cleanup: `verified`, `failed`, `unknown`.
Coverage: `complete`, `partial`, `unknown`. Hardware: `physical`, `simulator`,
`emulator`, `browser`, `host`, `unknown`. Warm state: `cold`, `warm`, `hot`,
`unknown`. Metric direction: `lower`, `higher`, `neutral`.

```sh
testrun diagnostics start INVESTIGATION --submission-id baseline --source '{"kind":"import","path":"/private/tmp/baseline.json","format":"reproduction-v1"}'
testrun diagnostics analyze INVESTIGATION COLLECTOR --submission-id baseline-analysis --metadata '{"observationComplete":true}'
testrun diagnostics compare INVESTIGATION BASELINE_ANALYSIS INVESTIGATION CANDIDATE_ANALYSIS
```

Wait for each collection/analysis to settle. Set `observationComplete` only when
the producer's run actually covers the declared window; it is a caller claim.
Comparison requires complete collection/coverage, verified declared cleanup,
known identities/environment and at least two valid measurements per run. Failed
attempts, altered environments, fault cleanup gaps and cancellation remain
visible as incompatibility reasons. Compatible results report counts, mean,
median, min/max and candidate-minus-baseline median in the declared unit. They
do not establish causality, statistical significance or tail-latency guarantees.
An optional nonnegative `metric.uncertainty` declares an absolute per-sample
measurement-error bound in the metric's unit. Compatible results sum the two
declared bounds for `medianDifferenceUncertainty`; omitted bounds stay unknown.
This is not a statistical confidence interval and is never reduced by sample count.

Limits: 1,024 attempts per run, eight MiB raw/expanded, 64 KiB per row, 16 MiB
derived index, 16 evidence references per attempt, 32 instrumentation labels.
The declared run budget is at most 24 hours; imported budgets are not enforced
execution deadlines. Duplicate IDs, unknown fields, decreasing/out-of-budget
attempt times are refused. Runs exceeding their declared attempt/time budgets
remain inspectable and cannot produce an eligible comparison. Live acceptance and long-running
campaign execution remain separate work.
