# Incremental aggregate design

## Question

Can Lattice maintain counts, sums and similar reversible reductions from
committed item changes, with useful large-collection savings and bounded unused
and small-operation costs, while preserving arbitrary scanning selections?

## Status

### Owner adoption and product integration

**Decided: adopt and integrate incremental aggregates.** The owner authorizes
the recommended product integration after the bounded shared-host validation.
`watchAggregate` is implemented in the regular collection source with public
JSDoc types and detached read-only access. Product tests exercise commit-prefix
delivery, net work bounds, current-key initialization/recovery, bulk membership,
errors, abort and Promise handling. The internal result helper is pure; no lab
module is imported by product code. Reference and usage guidance describe when
to use incremental reduction versus scanning.

The adopted design preserves the no-cache approach and corrected indexed
initialization/recovery. Earlier measurements remain scoped to their actual
source hashes and hosts; quiet-host/tail guarantees are not inferred. Product
integration evidence does not rewrite the prior inconclusive discovery. This
research branch is closed, with no further run or admission task scheduled.

### Owner closure: incremental-aggregate research

**Decided: close this research branch as inconclusive for API acceptance.** The
owner ends further investigation and timing work. Aggregate product integration
and API adoption are not authorized by closure.

| Finding                                                                       | Disposition at closure                                                                                                                                              |
| ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Reversible reductions, committed operands, prefix delivery and error recovery | Preserved in the proposed contract and architecture; behavioral/work evidence belongs to the lab candidate                                                          |
| Synchronous Promise handling and existing declaration-check reuse             | Policy and evidence recorded; not added to the product aggregate API, which does not exist                                                                          |
| Initialization/recovery follows current keys using indexed traversal          | Corrected and behaviorally verified in lab candidate `9fad377…`; initialization allocation confirmation passes; recorded as a requirement for any later integration |
| Ownership classifier corrections                                              | Saved-capture controls pass within the recorded scope; fixture/probe fixes remain lab machinery                                                                     |
| Correctness, allocation and ownership                                         | Completed evidence preserved at its measured revision scope; no new checks claimed                                                                                  |
| Update benefit, setup/ordinary latency, break-even and timing confirmation    | Unresolved; no qualified-host timing evidence                                                                                                                       |

There is no `watchAggregate` implementation under product `src/`. The prototype,
measured revisions and original findings remain preserved under the existing
artifact policy. The prepared consolidated timing handoff is archival, not a
pending execution task. No automatic continuation, product API promotion or
prototype deletion follows. Reopening requires a new owner decision, a suitable
reserved host and a bounded remaining comparison; later product integration
requires separate authorization and affected shipping-path evidence. Other
roadmap work is not blocked.

### Earlier status entries (historical)

**Consolidated preparation complete, 2026-10-27 UTC: recommend withholding the
aggregate candidate.** Correctness, allocation and ownership questions are
closed within their recorded revision scope. Timing benefit, ordinary/setup
costs and initialization break-even remain unmeasured. No quiet window or
alternate host is currently reserved, so no admission is attempted. The bounded
handoff's final budget extension is approved: aggregate ceiling 120 active
minutes, one bulk timing-only run at most 12 minutes, consolidated wall ceiling
30 minutes. The prepared adapter awaits a confirmed 35-minute quiet slot;
neither acceptance nor another run is automatic.

**Allocation question closed: both initialization cases pass, 2026-10-27 UTC.**
Exactly twelve profiles on corrected lab candidate `9fad377…` satisfy the
unchanged +1,024-byte criterion. Conservative extra-byte upper bounds are
−172,360 for count and −188,324 for sum. No extra samples, new heaps, test reruns
or timings. Original evidence remains tied to its measured revision. Timings and
break-even stay deferred; this does not accept the API or complete discovery.

**Saved-evidence diagnosis, 2026-10-27 UTC:** initialization allocations localize
to the aggregate's Map traversal and the fixture's scanning traversal, with a
smaller inspector/evaluation-boundary component. Numeric validation has identical
work in both sum paths and no direct sampled allocation site. A lab-only indexed
key traversal removes the Map iterator path and fixes a reproduced current-key-
order violation. Its allocation result is **not remeasured**. Two ownership
predicate corrections, reused from validated methods, pass all eight preserved
snapshot controls. No new profile, heap or timing run; API acceptance is unchanged.
The original discovery remains closed/inconclusive at its measured revision.

**Discovery closed inconclusive, 2026-10-27 UTC; do not advance or adopt yet.**
The lab-only prototype passes behavioral/reference-model and observed work gates.
Large relevant-update and unused-update allocation bounds pass. Initialization
is unresolved for count; sum exceeds the recorded +1 KiB sample-envelope gate,
without an established source attribution. Ownership calibration and exposed/
aggregate-owner release succeed, but the broader classifier fails positive
controls. No quiet window is reserved, so timings and break-even are unmeasured.
The single correction fixed a test oracle; no further correction, resampling,
confirmation or continuation occurs. Bulk source `f713077…` remains unchanged.

| Requirement                                                        | Current evidence                                                                                                                   |
| ------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------- |
| Behavior and work bounds                                           | Original lab gates pass; indexed initialization/recovery correction also passes its order regression and affected behavior         |
| Large count/sum relevant-update allocations                        | Pass against the proposed bounds                                                                                                   |
| Unused ordinary-update allocations                                 | Pass against the proposed bound                                                                                                    |
| Initialization allocations                                         | Corrected revision passes count and sum confirmation; original count inconclusive and sum observed failure remain preserved        |
| Ownership                                                          | Narrow predicate correction passes all eight preserved cohort/release controls; original byte accounting unchanged, no new capture |
| Relevant benefit, ordinary timing costs, initialization break-even | Unmeasured; no reserved quiet host or timing attempt                                                                               |
| Independent confirmation                                           | One bounded initialization allocation set complete; timing benefit and break-even confirmation still deferred                      |

**Approval and limits, 2026-10-27 UTC.** The owner authorizes the recorded
scope and approximately 70 minutes remaining after preparation. Resumed active
work starts at 00:47:55 UTC, with a 01:57:55 UTC ceiling for this continuation;
no timing window is reserved. Correctness/work gates precede all measurement.
Independent allocation/ownership work proceeds if those gates pass. At most one
targeted correction and one confirmation; no matrix expansion or auto-continuation.

The approved 90-active-minute scope covers preparation, prototype work,
calibration/fixes, measurement, correction, confirmation and reporting. The
lab-only prototype and measured observations are preserved; product source and
pending bulk acceptance stay unchanged. The
[aggregate contract](../specs/javascript/contracts/incremental-aggregates.md)
remains a proposed product API. Approved discovery criteria are not shipping
guarantees or a decision to accept that API.

Review clarification: reject thenable computational results through explicit
phase-specific error handling, reuse existing `checkSelection` for aggregate
projections including empty declarations, and require safe-integer bounds at
every exact-sum intermediate. Those behaviors are exercised in the lab;
product implementation and shipping acceptance remain unapproved.

## Hypothesis

A framework-owned accumulator with no per-item contribution cache can project
before/after commit records and update in proportion to changed items. It should
save full scans for large single-item updates without adding O(items) retained
state per aggregate. Two projections per relevant update may be cheaper overall
than maintaining that cache. Initialization still scans every item, and an
additional method/commit integration has construction and ordinary-operation
costs that need measurement. These are design hypotheses, not results.

## Method

### Consolidated final timing handoff (2026-10-27 UTC)

**Approval and preparation amendment.** The owner approves the aggregate
cumulative ceiling of 120 active minutes, one 12-minute bulk timing-only run
and a 30-minute consolidated execution ceiling. Preparation begins 02:16:48 UTC;
charge its five-minute allowance, including review and reporting, to the same
aggregate ledger: estimated cumulative **91.914 of 120 minutes**, approximately
28.086 remaining. This replaces no historical expenditure. No further extension,
matrix expansion or automatic retry. No quiet slot is confirmed at preparation
time; no admission or timing run occurs.

The narrow timing adapter and sequential coordinator are now prepared. The
four runner files pass syntax checking. Their first behavioral smoke and clock
checks remain inside the admitted timing phases, not an unreserved run. Initial
and confirmation phases execute Node, Chromium, then fresh Node and Chromium
confirmation in the recorded order. A bulk observed gate failure does not
erase its completed evidence or prevent independent aggregate work while host
admission remains valid. The same host monitor continues across candidates.

Large aggregate batch/membership timings remain reported tradeoffs as originally
specified; no new percentage or tail gate is invented for them. Existing
small/unrelated, unused and large-single-change gates and break-even remain
unchanged. Any incomplete smoke/phase yields withholding, never another attempt.

**Owner scope.** Finish bulk membership and aggregates as separate decisions
using one serial timing session. Reuse closed correctness, allocation, consumer
and ownership evidence at its recorded revisions. Reopen one of those questions
only for a concrete affected source change or contradictory evidence. No runtime
edits, new variants, workload matrix, profiler, heap investigation or API design.
The [bulk timing handoff](collection.md#stage-6-future-timing-handoff-2026-10-26-utc)
owns its unchanged cells and numerical gates. Bulk candidate/control are
`f713077…` / `1cbd0b3…`; aggregate candidate/control are `9fad377…` / `f713077…`.
Preserve all sources and earlier evidence; do not combine samples across revisions.

**Budget feasibility and requested amendment; not approved.** The aggregate
ledger entering this task is approximately 80.914 of 90 active minutes, leaving
9.086. Charge this handoff a rounded six active minutes, including review,
planning and closeout: approximately **86.914 spent, 3.086 remaining**. Waiting
for the owner or host is excluded. Preparing and executing the consolidated
session cannot reliably fit that balance. Request **one final extension of 30
aggregate active minutes**, changing its cumulative ceiling from **90 to 120**
and leaving approximately 33.086 after this handoff. Charge timing-adapter
preparation up to five minutes, aggregate timings up to twelve, and aggregate/
shared analysis and final reporting up to eight: **25 active minutes maximum**
after approval, limited also by the cumulative 120-minute ceiling. No unused
time funds samples, corrections or another attempt.

The bulk ledger records a future **12-minute per-run ceiling**, not an auditable
single cumulative active-effort balance. The last quiet-window attempt spent
60.058 seconds; that and all earlier expenditures remain spent. To avoid
silently treating a per-run cap as renewed cumulative authorization, the same
single amendment explicitly requests **one bulk timing-only execution allowance
of at most 12 minutes**, preserving every existing phase cap. No claim is made
that historical bulk spending resets or that unmeasured work was free.

**One execution, in this order.** Overall consolidated wall ceiling **30 minutes**
from coordinator launch, including transport, cleanup and reporting. One host
admission attempt, at most 60 seconds, using the existing eight five-second
qualified windows and thresholds; continuously monitor through both candidates.
No renewed admission between candidates, no wait/retry after failure or loss.
Run bulk first, close all its target processes, then run aggregate timings with
one owned target at a time. Shared admission is charged to bulk; do not charge
it twice or extend the bulk cap. Clear bulk timing failure may stop that
candidate's unnecessary work while aggregate work proceeds under still-valid
host admission; host loss stops all remaining timings. Preserve partial evidence.

| Phase                                                | Fixed scope and sampling                                                                                                                                                                                                                                                  | Ceiling                                                                                                       |
| ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Admission, bulk timings and bulk report/cleanup      | Existing Node/Chromium runner: four bulk cells, two sets/runtime, one warmup plus seven actions/variant/cell; ten small controls with 3 warmups/15 blocks, second set only direct/batched append; three setup cells with three processes/runtime and 5 warmups/100 blocks | Bulk 12 minutes total; admission 60 s, bulk 45 s, small 150 s, setup 240 s, report/cleanup 120 s; no transfer |
| Aggregate Node timings                               | Original 22 paired cells, unchanged fixtures and boundaries; 3 warmup blocks and 15 measured blocks/variant/cell; 64 independent small fixtures or four large fixtures/block                                                                                              | 4 minutes including launch, clock smoke, preparation, teardown and saved results                              |
| Aggregate Chromium timings                           | Same 22 cells and counts in the browser environment already identified as required timing acceptance; offline isolated browser clock and serial transport reused                                                                                                          | 4 minutes, same inclusions                                                                                    |
| Aggregate Node confirmation                          | Original seven-cell confirmation: large count/sum initialization and relevant update, unused construction/update, worst small relevant cell by median overhead ratio (count wins a tie); fresh process, same warmups/blocks                                               | 2 minutes                                                                                                     |
| Aggregate Chromium confirmation                      | Same seven-cell rule and counts in a fresh browser realm                                                                                                                                                                                                                  | 2 minutes                                                                                                     |
| Consolidated reporting/cleanup and transition margin | Per-candidate requirement tables, source hashes, cleanup and owner recommendations; no new measurement                                                                                                                                                                    | Remaining 6 minutes of 30-minute wall ceiling; reporting itself at most 3 minutes inside the run              |

Bulk expected duration remains six to ten minutes. Aggregate fixed-chunk timings
have no observed runtime baseline: estimate four to eight minutes total, with
twelve as the hard maximum. Expected consolidated reservation is **15–24 minutes
after launch**, with a **35-minute quiet slot** allowing five minutes settling.
These are feasibility estimates, not measured aggregate performance. Per-phase
ceilings take precedence over completion; never enlarge counts or caps. First
clock/action smoke can establish that completion is infeasible; preserve the
gap rather than calibrating again. No fourth setup replica or second confirmation.

**Unchanged rules.** Bulk retains membership median ratio ≤0.50 beyond both IQRs;
mixed/ordinary/setup median allowance max(5%, 0.5 µs); small bulk allowance
max(25%, 2 µs); observed tail allowance max(10%, 2 µs). All three clean setup
replicas must pass; conflict is insufficient evidence. Retain original 20 ms
adequacy, block/action caps and conservative clock-bound rules.

Aggregate retains large relevant-change median ratio ≤0.50 with conservative
clock allowance and improvement beyond both IQRs; small active/unrelated median
allowance max(25%, 2 µs); unused construction/update allowance max(5%, 0.5 µs).
Compute initialization break-even at N=10,000 separately for count and sum in
each engine: nonnegative conservative extra initialization cost divided by
conservative saved relevant-update cost, rounded up, **at most 100 relevant
commits**. A nonpositive/unresolved saving denominator leaves break-even
unresolved. Keep clock-pair accounting and matched work; no accumulation until a
preferred result appears. Three warmups/15 blocks remain fixed; p95 is an
exploratory block-average maximum, not an independent tail acceptance claim.
Do not import a new bulk tail gate into aggregate discovery. Independent
confirmation must support every claimed timing pass; conflicts withhold.

**Original preparation estimate, superseded by the approved preparation above.** The bulk timing-only command is already
prepared in its linked handoff and stays unchanged as the standalone baseline.
The aggregate's existing probes/worker currently implement allocation/ownership,
not the planned timing cells. A consolidated command is therefore **not yet
executable**. Within the proposed five-minute adapter allowance, reuse the bulk
runner's host/admission/clock/cleanup/report mechanisms and original aggregate
fixtures/actions; wire sequential execution under one admission and deadline.
No unvalidated timing claim follows from this plan or from the existence of an
allocation runner. If this narrow adaptation/smoke cannot complete within its
allowance, withhold aggregates; do not invent an ad-hoc replacement or extend.
Record the exact consolidated command and runner hashes before execution.

The approved preparation now provides the exact command below, from the
workspace root, **only after the 35-minute slot is confirmed and competing
workloads are paused**. The explicit flag is an owner-confirmation guard, not
permission inferred from the earlier expired reservation:

```sh
node packages/lattice/labs/bulk-membership/quiet-window-run.js --quiet-window-confirmed --timings-only --allocation-evidence packages/lattice/labs/bulk-membership/results/2026-10-26T23-54-58-142Z-bulk-allocations/report.json --consolidated
```

The package candidate/control and closed evidence hashes stay unchanged. The
new coordinator archives all measured adapter/fixture sources and checks for
drift at completion. No allocation or heap phase is reachable in this mode.
Prepared source SHA-256 hashes:

| Source                   | SHA-256                                                            |
| ------------------------ | ------------------------------------------------------------------ |
| Sequential coordinator   | `5d6e8a77f5d9e0ddd794365d5f77a871fa349fbc8656f9a1ab20ae6ff1f8aab3` |
| Aggregate timing probes  | `a01deeef6a0b17e5bb4df42b527949f4b9343c28f5a0df28e73f71d5508c4033` |
| Aggregate timing worker  | `68295035d760e1929b267327b7799da20cb31fa5fc92534ededa5c36f2945bce` |
| Aggregate timing adapter | `4946717c49ddabfef1d440aa66d10618fe1dbb8ad45feabecc6279883e7456c2` |

Expected reservation and all ceilings above remain unchanged. Until actual
timing evidence exists, both recommendations remain **withhold for unresolved
measurements**, without attributing intrinsic regressions or promoting APIs.

**Resources and stops.** One coordinator, at most one target Node child or one
headless Chromium tree, four GiB free RAM, one GiB artifact space, installed
dependencies/browser; no installs, network services or profilers. Maximum
100,000 prepared live items, fixed matched chunks. Only an explicitly reserved
quiet window with competing workloads paused, or an owner-selected suitable
host, permits launch. The earlier reservation does not carry forward. None is
currently supplied: finish this handoff and stop without even admission. No
automatic rescheduling, retry, integration, API promotion or commit follows.

**Decision and conditional closeout.** Recommend acceptance separately only if
each candidate's valid recorded requirements and its outstanding timing gates
are established. Report observed gate exceedances separately from attributable
regressions; contention, inadequate precision and conflicting clean replicas
withhold evidence without proving intrinsic regression. Aggregate lab timing
success can support acceptance of the approach but does not itself verify a
shipping implementation. After an owner acceptance decision, any remaining
aggregate integration, affected shipping-path correctness/consumer/timing
verification and public examples/docs require a bounded closeout scope; do not
claim those unmeasured costs fit this timing-only amendment. Bulk is already
integrated, so avoid a duplicate implementation. Inspect the entire staged
index before each scoped closeout commit, preserving other sessions' work.

### Bounded initialization allocation confirmation proposal (2026-10-27 UTC)

**Approval amendment.** The owner approves exactly twelve profiles and requests
closure afterward. Charge all follow-up work to the original 90-active-minute
budget, without resetting it. The additional 15-minute cap is a sublimit of that
budget. The prior discovery ledger records approximately 45.914 active minutes;
record intervening diagnosis/planning effort separately rather than treating it
as free or restarting the ledger. Reuse valid evidence; no extra samples, timings,
break-even measurement or API acceptance.

**Executed scope.** Active work starts at 02:02:08 UTC. The coordinator collects
exactly twelve profiles in one set. The initial preflight aborts before browser
launch or sampling because the new source guards duplicate two source-archive
paths; deduplicate that list, preserve the failed directory and proceed within
preparation. No candidate/probe/fixture correction or extra sample is involved.

**Decision and hypothesis.** Determine whether the corrected initializer meets
the existing initialization allocation discovery criterion for count and integer
sum. Saved stacks identify aggregate iterator work as the dominant sampled
cost. Indexed current-key traversal removes that path; reduced allocation is
an expectation, not a confirmed result. The ordering correction is independently
verified behavior and does not depend on a favorable allocation outcome.

**Variants and scope.** B0 remains the unchanged collection at SHA-256
`f7130777350dde04d1e11bc9bf6e4843418a86161d882b669ca2d0913291dd74`,
using the existing scanning selection. B1 is the lab-only aggregate candidate
`9fad37793eab029396b9f70069e77aea1ab316cc8f9063ffae899a815c330f2a`.
The synchronous-result helper remains `e9029de…`. Record the public repository
revision and full hashes of both variants, helper, fixture, probe and runner;
archive measured sources and check them again at completion. Source drift stops
confirmation. Keep original candidate `8d9e439…`, profiles, failed sum envelope
and inconclusive count envelope unchanged. No third variant or product edit.

| Case                       | Matched work per profile                   | Warmup per variant                                    | Profiles per variant |
| -------------------------- | ------------------------------------------ | ----------------------------------------------------- | -------------------- |
| Count initialization       | One new watcher over 10,000 existing items | One complete unprofiled registration/validation/abort | 3                    |
| Integer-sum initialization | One new watcher over 10,000 existing items | One complete unprofiled registration/validation/abort | 3                    |

These are the two affected original cells, not another discovery matrix. Reuse
the deterministic fixtures, integer validation, scanning implementations and
`allocationPrepare` / `allocationApply` / `allocationRelease` probes unchanged.
Use a fresh owned Chromium profile with the existing offline browser transport.
Run count then sum, alternating B0/B1, B1/B0, B0/B1 across the three measured
rounds of each case. Prepare one fresh equivalent collection per profile and
release it before the next. Maximum simultaneous fixture volume is 10,000 items.
The browser stays warm across cells, matching the original method; these are
fresh registrations, not cold-process startup measurements.

**Measurement and correctness boundaries.** Reuse Chromium CDP allocation
sampling at 512 bytes, including objects collected by major and minor GC.
Construct records, collection, caller descriptor, signal, callback and options
before starting sampling. Sample the complete public watcher registration,
initial scan/reduction and initial delivery. Stop sampling before full
recomputation, error assertions and abort cleanup. Initial callbacks only store
the delivered result. Keep the complete sampled profile total as bytes per
registration; do not subtract the inspector boundary or iteration sites.
Full-stack attribution may explain the result but cannot change its gate.
One completed registration is the sampling unit; no action accumulation or
calibration growth. Reuse the original validated sampling mechanism. Smoke each
case/variant with the unprofiled warmup, checking exact recomputation and no
reported errors before collecting that case. Stop on correctness, transport or
cleanup failure, preserving earlier complete profiles.

**Decision rule.** Keep the proposed initialization limit at **+1,024 sampled
bytes per registration**. For each case report every profile, n, min, median,
nearest-rank p95/max and IQR for B0 and B1. Compute the same conservative observed
extra-byte envelope: lower = min(B1) − max(B0), upper = max(B1) − min(B0).
Upper ≤ 1,024 passes this discovery criterion; lower > 1,024 is an observed
criterion failure; otherwise the cell is inconclusive. Three profiles do not
establish statistical confidence or an allocation tail; p95 equals the maximum.
Both cells must pass to confirm this particular allocation correction. An
incomplete cell remains unresolved. Do not pool old and corrected revisions,
change the metric, discard unfavorable profiles or add samples to resolve noise.
Passing these cells does not establish API acceptance or timing break-even.

**Evidence reuse.** Reuse the corrected revision's passing initialization/recovery
order regression and affected Node/Chromium behavior; do not rerun unchanged
covered source. Reuse the repaired classifier's eight saved-snapshot results,
including exact live/empty/released cohorts and unchanged byte accounting,
explicitly scoped to measured candidate `8d9e439…`. Source review of the indexed
loop introduces no persistent owner; this supports reuse, not a claim of new
heap measurement on `9fad377…`. No classifier work, replay or fresh capture is
planned without a specific new framework-ownership concern. Preserve prior
relevant-update and unused-update allocation evidence for their unchanged paths.

**Budget, resources and closure.** Propose one **15-active-minute** follow-up
ceiling, covering source review/runner preparation (5 minutes), smoke and sampling
(up to 90 seconds), and analysis/reporting (up to 8.5 minutes). This is a bounded
follow-up to the closed discovery, not a reset or expansion of its original
90-minute ledger. Reuse the allocation runner with an explicit two-cell-only
selection and no ownership phase; its adaptation is charged to preparation.
No runtime correction, second profile set, dependency installation, network
access or automatic continuation is authorized by this proposal.

The original full 30-profile allocation phase took 3.426 seconds; expect this
12-profile subset to take seconds to tens of seconds including launch and setup,
with a **90-second coordinator ceiling** covering launch, smoke, collection and
cleanup. Use one Node coordinator and one owned headless Chromium process tree;
reserve 4 GiB RAM and 128 MiB artifact space, with no heap snapshots. Persist each
completed profile into a new dated directory without overwriting earlier results.
Close owned browser resources on completion, cancellation and failure; preserve
partial evidence and report cleanup failures if the ceiling is reached.

Allocation sampling is independent of timing admission: record host/runtime/load,
but do not attempt timing admission or infer timings from coordinator duration.
No sensitive timings, break-even run or quiet-window retry. Host waiting does
not spend active effort. Finish with pass/observed failure/inconclusive for each
of the two cells and the exact remaining timing requirements. Any unresolved
allocation evidence stays unresolved at the cap; no automatic matrix expansion.

### Saved-profile attribution and narrow corrections (2026-10-27 UTC)

The owner separately requests source/saved-profile diagnosis, a concrete
correction or explicit measurement gap, reuse of validated ownership methods,
and no timing campaign or API adoption. This is not another discovery run or
a reset of that run's one-correction ledger. Preserve original source, reports,
sample criteria and classifier failures. No performance data is collected.

Partition all twelve saved initialization profiles by their **full allocation
stack**, not function-name totals alone. Attribute native `next`/`values` beneath
`rebuildAggregate` and rebuild self charges to an aggregate iteration cluster;
control `next` beneath scanning `watch` to its scan cluster. Keep direct numeric
validation, other registration/guard work and allocations outside
`allocationApply` separate. The last category is an inspector/evaluation boundary,
not established application allocation. A diagnostic subtree partition does not
replace the original complete-action metric or retroactively change its gate.

Preserve candidate `8d9e439…` before replacing only its Map-values initialization/
recovery loop with indexed traversal of the existing immutable keys and `Map.get`.
Before correction, a behavioral reproduction with key order `c,a,b` visits
`a,b,c`, proving Map insertion order violates the proposed initialization order.
Verify initialization and dirty recovery order and affected behavior after the
correction. No collection construction, persistent state, numeric validation,
ordinary product operation or API changes.

Preserve the old ownership graph and reuse the validated method's actual string-
key predicate to exclude Hole-valued templates. Recognize the fixture's records
by existing nonnumeric `done`/`label` fields, excluding Hole placeholders, rather
than requiring an absent numeric edge. These are **two predicate corrections**
within the existing traversal; no new broad classifier, snapshot-ID matching,
root accounting or byte metric. Replay the eight original captures, check hashes,
and require exact known live/empty/released cohorts and unchanged byte accounting.

### Small discovery proposal (2026-10-27 UTC)

**Question and scope.** Decide whether the no-contribution-cache design merits
integration/acceptance work, and identify any concrete ordinary-path cost.
This is discovery, not adoption. Do not repeat bulk membership acceptance,
add an index, generalize reducers, or reopen its retention investigation.

**Variants.** B0 is the unchanged regular collection entry at SHA-256
`f7130777350dde04d1e11bc9bf6e4843418a86161d882b669ca2d0913291dd74`,
using scanning `watch` selections. B1 is an ignored lab-local extension of that
same frozen source implementing the proposed `watchAggregate` integration.
Use ordinary insert/remove/batch in both; do not depend on unaccepted bulk
membership. Pin every measured source/helper/reporter and preserve revisions.
No product API/export or runtime flag is added by lab approval. A later product
checkpoint and affected shipping-path confirmation need separate authorization.

**Inputs.** Seed 1, neutral records `{ done, cents, label }`, N=10 and N=10,000.
Initial `done` is true every third item; integer cents stay in a small bounded
range. Labels use shared literal values, not per-action concatenated strings.
Run one subscription/reduction at a time. Count projects `done ? 0 : 1`;
sum projects `cents`. All exact-sum contributions, seeds and arithmetic
intermediates pass `Number.isSafeInteger`. Both variants use the same immutable
records, signal lifetime, field dependencies, result equality and action input.

| Cases fixed before measurement       | Count                                         | Integer sum                                   | Boundaries                                                                                                                     |
| ------------------------------------ | --------------------------------------------- | --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Initialization, N=10 and 10,000      | Both sizes                                    | Both sizes                                    | Fresh collection and signal prepared; time registration plus initial callback only; measure collection construction separately |
| One relevant change, both sizes      | Toggle one `done`                             | Change one `cents` by 7                       | Complete public write including aggregate delivery                                                                             |
| Unrelated change, both sizes         | Change `label`                                | Change `label`                                | Complete write; zero reduction/projection work expected                                                                        |
| Batched relevant changes, both sizes | Change min(N,100) items                       | Change min(N,100) items                       | One outer batch and one final result; counts use actual net changes                                                            |
| Membership, both sizes               | Remove/reinsert one key with new contribution | Same                                          | One ordinary remove/insert batch; replacement incarnation, constant collection size                                            |
| Unused feature controls, N=10        | No aggregate or scan subscription             | One shared control, not duplicated by reducer | Fresh construction and one ordinary changing write, separate cells                                                             |

This is **22 paired cells**, not a Cartesian sweep of sizes, update counts,
subscription counts and runtimes. Reorder, empty fields, unchanged writes,
multiple subscriptions and larger membership groups belong to correctness/work
tests here, not extra timing cells. No N=1,000 or additional scales are added
after seeing results.

**Behavior and work counts first.** Before profiles or timings, run prototype
tests through B0/B1 factories and the existing test runner. Compare every
generated committed result against a separate straightforward full fold over
a reference model. Include a fixed 200-action seed-1 sequence at N=10 and N=100;
check after each commit, never only at sequence end. For reentrant tests, the
reference model records committed prefixes; do not incorrectly expect scanning
watchers' later live views to have the new API's prefix semantics.

Assert initialization uses N projections and N additions; a relevant update
uses at most two projections, one removal/addition and one delivery; unrelated
and reorder-only changes use zero projections/reducers/deliveries; a batch
processes only net changed operands once. Normal membership replacement uses
one old and one new projection. Callback counters are observed in this phase
and disabled in measurements. Audit internal loops too: callback counts alone
cannot prove there is no hidden collection scan.

Behavioral gates include nested/thenable rollback; inserted-updated and
updated-removed items; reused keys; no-net changes; initialization/abort cleanup;
registration during delivery; and queued `0 → 1 → 2` delivery despite earlier
callbacks writing again. Inject projection/add/remove errors after partial work:
the model commit stands, no partial result publishes, the next relevant commit
rebuilds once, and unrelated commits do not retry. Equality/run errors keep
healthy state and last-successful-delivery semantics. Test exact-sum intermediate
overflow refusal. Reuse `checkSelection`, including empty declarations; no new
checker. Stop before measurements if correctness or work bounds fail.

**Promise policy.** Actual Promises are rejected synchronously after attaching
discarding fulfillment and rejection handlers through the captured native
Promise operation. Both handlers return undefined; no original/derived rejection
is left unobserved, no settlement becomes an aggregate value, and no own `then`
override is invoked. Non-Promise thenables are detected but not invoked. The
isolated helper is not a full result validator or evidence for aggregate error
routing. Integrate and test rejection from every computational callback, both
initialization and maintenance, and equality delivery; assert dirty/healthy state,
guard restoration, exactly one aggregate error and no later result/rejection.
Use an owned strict-unhandled-rejection Node child plus its failing unobserved
control. Chromium tests observe `unhandledrejection` without preventing default.
Cover fulfilled, already rejected, pending-then-rejected and cross-realm Promises,
own `then` overrides and arbitrary thenables. No global rejection suppression.

**Reuse and harness validation.** Reuse the bulk harness's serial Node/Chromium
host ownership, atomic raw reports, source pins, field-count wrappers, Chromium
512-byte allocation sampler including collected minor/major objects, native
abort ownership and corrected memory-probe principles. Do not reuse its action
oracle, membership-only cases, private-owner classifier or obsolete snapshot-ID
comparisons as aggregate evidence. The aggregate record/queue seam and error
recovery need new smoke cases. The preparation budget includes runner adaptation.

Smoke one matching action per variant/cell, record preparation/action/reset/
verification wall cost separately and estimate the full run before sampling.
Validate the clock with 1,000 adjacent reads and a fixed empty matched chunk;
do not calibrate by increasing repetitions until a cheap control looks precise.
Require a known retained/released 128 KiB probe before attributing retained bytes.
A failed mechanism stays unresolved; it does not consume another calibration
campaign. Any correction spends the one correction allowance below.

**Discovery timing, Node only.** After warmup, three warmup blocks and 15
measured blocks per variant/cell; alternate first variant with seed-1 ordering.
Each block is one fixed matched chunk: 64 independent N=10 fixtures or four
N=10,000 fixtures. Setup, resets, counters and full-recomputation checks precede
or follow the clock; registration/action and synchronous delivery are included.
No equal-work division can substitute for matched fixture volume. At most
40,000 live large-fixture items are prepared at once; use the unchanged
100,000-item hard preparation ceiling. Report µs/action, block count, p50,
p95/max, IQR, setup and teardown separately. P95 of 15 block averages is
exploratory, not individual-action tail latency. Clock uncertainty uses every
actual timer pair and the maximum observed empty span; fixed counts never grow.
If conservative bounds cannot resolve a proposed threshold, mark it inconclusive.
Chromium timing acceptance is a later requirement, not inferred from Node.

**Allocation and retention discovery, independent of timing admission.** Sample
only large count/sum initialization, large count/sum single changes and unused
ordinary update (five cells). Three profiles per variant/cell, fixed before the
run, at most 30 discovery profiles. Use one fresh large initialization or a
prepared chunk of four independent large updates/profile; use 64 small unused
updates/profile, and normalize per named action. Preserve all profiles, with
cross-profile extrema; straddling bounds remain inconclusive, not permission
to collect more. Preparation/oracles/teardown are outside sampling.

Retained-memory evidence is limited to **eight Node snapshots maximum**:
known retained/released calibration (two); actual B0/B1 with one live watcher
at N=10 (two), live at N=10,000 (two), and after abort/drop at N=10,000 (two).
Warm both paths before the measurement baseline; keep equivalent inputs,
probe lifetimes and task/GC boundaries. Count actual aggregate-owned nodes and
retaining paths, check live-owner positive controls, distinguish application
storage from code/runtime footprint and fixture-held objects. After abort/drop,
require zero obsolete private aggregate/queue/callback owners. A weak-reference
check complements snapshots, without comparing IDs across captures. Extra
live application storage should remain O(1) in item count; report the byte delta
and raw total separately. If attribution/calibration is inadequate, retain an
explicit memory gap; no extra heap captures or broad leak investigation.

**Proposed decision criteria, not inherited bulk gates.** Correctness and stated
work bounds must pass. For both large single-change reducers, candidate median
with conservative clock allowance must be at most half control and improve
beyond both IQRs. Small active and unrelated actions must stay within the larger
of +25% or +2 µs; unused construction/update within the larger of +5% or +0.5 µs.
Report initialization tradeoff and a conservative break-even count from added
initialization cost divided by saved relevant-update cost; proposed bound is
at most 100 relevant commits at N=10,000. Unresolved denominators stay unresolved.

Proposed allocation bounds: no more than 256 extra bytes per relevant update,
64 per unused ordinary update, and 1 KiB extra per initialization. These are new
discovery proposals, not previously approved product limits. Cross-profile bounds
must establish them. Retention's deterministic release gate is mandatory; a
well-attributed retained-application excess over 16 KiB per watcher, or evidence
of a new per-item cache, fails the intended no-cache design. Unattributed VM/code
variation is a measurement limitation, not a leak. Discovery may identify a useful
candidate with memory bytes unresolved; it must carry that acceptance gap and
cannot claim adoption. Membership/init costs, allocations and tails are reported
even when they favor scanning; do not silently drop a cell.

**Bounded correction and confirmation.** Permit one targeted correction total
(runner or candidate), supported by a concrete failing test, work count or
allocation/owner site, within its budget. Preserve the failed revision. Rerun
only affected behavioral/work cases and already declared affected cells; if a
shared change requires broader evidence than the budget permits, close with a
specific gap. No redesign or restarted discovery matrix.

Confirm only if initial evidence can support advancing the candidate: one fresh
independent Node process for large count/sum relevant change and initialization,
unused construction/update, and the worst small relevant-change cell selected
by the predeclared median overhead ratio (count wins a tie): seven paired cells,
same 3 warmups/15 blocks. At most three allocation profiles/variant for each
large relevant-change reducer (12 confirmation profiles maximum). No further
heap captures. Correctness/release failures or clear ordinary-path gate failures
stop without confirming a rejected revision. Conflicting replicas or remaining
clock/sampling limits close as inconclusive, without a third run set.

**Host conditions.** No sensitive timing attempt until an owner-reserved quiet
window or a separately selected suitable host. Attempt the existing eight
five-second admission windows once, at most 60 seconds: idle ≥30%, normalized
load ≤0.75 and busy range ≤15 percentage points; monitor during timing. Lost
admission taints overlapping cells and stops later timings, without retry or
implementation rejection. Continue independently valid behavior/allocations/
retention only within the total budget. Host wait with no owned run is not
active effort; it does not authorize an indefinite investigation. If a suitable
host is unavailable, finish non-timing evidence and close discovery inconclusive
with the timing requirement named, rather than launching repeated admissions.

**One total budget: 90 active minutes.** Proposed allocation includes this plan
and isolated policy-test preparation (25 min), candidate plus behavioral/work
tests (25), harness adaptation/smoke/calibration (8), discovery evidence (12),
one targeted correction (8), independent confirmation (5), and analysis/report
(7). These are planning allocations within one shared ceiling, not new budgets
after fixes; record actual effort already spent and the balance before approval.
No phase or correction resets the clock. Any unused confirmation time may fund
reporting, never extra cases/profiles. On exhaustion, preserve useful changes and
close inconclusive with exact missing evidence; do not discard the bulk candidate.
No further cycle without a new owner priority decision and feasible hypothesis.

Preparation charged so far is approximately **20 active minutes**, an effort
estimate rather than a benchmark measurement; approximately **70 minutes remain**
of the same 90-minute proposal. Approval does not reset that balance. Track
subsequent active work and owned run wall time separately from waiting for the
owner or a suitable host, and reconcile actual totals when closing the record.

**Outcome.** Recommend advance to a separately scoped integration/acceptance step
only if the primary benefit and ordinary-path criteria survive confirmation and
no correctness/ownership defect remains. Record memory or engine gaps explicitly.
Recommend reject the discovered approach on established correctness/cost failures;
otherwise close inconclusive. Neither a useful prototype nor a discovery timing
win accepts an API or unmeasured product requirements.

First review `watchAggregate({ fields, read, initial, add, remove, equals? },
run, { signal })`, its order-independent reversible reduction laws, committed
prefix delivery and dirty recovery. The contract example is proposal notation,
not a public runnable recipe. Ordinary `watch` remains the full-recomputation
control and the solution for derivations outside these laws.

Source review covers collection commit/queue operands, rollback normalization,
membership/item/wide ordering, registration versions, abort/error handling and
task-example scanning selections. The current queue's membership metadata has
keys without immutable old/new record operands; a public-watcher wrapper cannot
reliably reconstruct every queued prefix from the later live collection view.
That is an integration constraint, not a diagnosed existing bug.

Proposed implementation boundary: normalize immutable changed record operands
only while aggregate subscribers exist, maintain accumulators synchronously at
each committed boundary before callbacks, and queue immutable result values in
the existing wide-delivery order. No public delta stream, per-item subscription,
contribution cache, retained full snapshot or aggregate getter is proposed.
Ordinary insert/remove/batch must work independently of unaccepted bulk membership.

The callback result check reuses the shared `isThenable` rule for arbitrary
thenables and adds native Promise rejection observation as specified above:
never await or accept its value, and route failures by phase.
Initialization failures propagate without registration; maintenance failures
mark/stay dirty and report after the model commit; equality failures report
without dirtying healthy state or advancing last-delivered values. A throwing
`then` getter on an arbitrary thenable follows the callback's thrown-error path.
Observation prevents an actual Promise's later unhandled rejection, without
owning or cancelling asynchronous work the callback already started.

Source review of test support confirms `checkSelection` calls the shared
`checkCases` with item-record proxies and accepts an empty field declaration.
Use the reduction's `{ fields, read }`; a constant empty-fields reader passes,
while any known/unknown item-field read is undeclared. Empty cases still fail.
No new checker is necessary; dedicated record-checker empty-field coverage and
consumer structural typing are implementation acceptance gaps. Dependency
checking does not establish synchronous returns or arithmetic correctness.

Exact sum fixtures and example reducers must ensure contributions, the seed
and every initialization/recovery/update intermediate remain safe integers,
including temporary remove-old/add-new values. Checking only final totals is
insufficient. Validate this domain in the numeric reducer rather than inferring
numeric rules for arbitrary framework accumulator types.

The discovery proposal above chooses count and integer sum as its measured
reductions. Structured accumulators and additional scales are not extra timing
cells. API/prototype scope, numerical gates, host conditions and the one total
budget still require agreement before the comparison. No performance experiment
or heap work is authorized merely by writing this record; bulk-retention closure
remains unaffected.

## Conditions

### Product integration source identity

The owner-authorized integration uses collection source SHA-256
`63d78b7d8c0a2609adfad9e14f0e641db862eafe7572b2599b91300478f7dce1`
and internal result-helper SHA-256
`e63675065644a337dea3d1c869c6825f532a98500caf8e56ccd7765360d568ed`.
It includes deferred unlink during aggregate maintenance and a single ordered
cursor for queued results. Product regression cases cover aborting an earlier
aggregate without skipping later owners and inactive/unaffected owners without
misaligning delivery. These are integration corrections, not measurements of
new timing guarantees. Original lab-source identities below remain unchanged.

### Initialization allocation confirmation (2026-10-27 UTC)

Public repository revision `f9aed61b29f0250664d35cb5fe0ab4b283ea00d1`,
unchanged control `f713077…`, corrected candidate `9fad377…`, result helper
`e9029de…`, fixture `95992e10…` and probe `1fd2901d…`. Full source copies and
hashes are preserved locally. Runner hash is
`5ff7ccab173e7b1ff29eab22ffce3d77bec318d9ddfd7aa23b2464355ac265f3`;
final source-drift check is empty. Raw report hash is
`0104abd30a948745a114b0c31b72ad75eb03a065e53047b071ad9bb736176a8b`.

Node 24.12.0 coordinator; Chromium 156.0.8078.4; Darwin 24.6.0 x64;
Xeon W-2140B, 16 logical CPUs, 64 GiB RAM. One fresh offline browser profile,
one browser process tree, unchanged sampling and lifecycle boundaries. Starting
load averages are 9.95/15.54/15.24 and ending averages 9.47/15.35/15.17. This is
allocation sampling under contention, not timing evidence or quiet-host admission.
No network requests escape the offline route. Browser cleanup reports no errors.

### Approved discovery conditions (2026-10-27 UTC)

The unmodified product control has SHA-256
`f7130777350dde04d1e11bc9bf6e4843418a86161d882b669ca2d0913291dd74`;
the lab-only candidate is
`8d9e439069641486f1232b616fcf18e88b739a27e91189908c7ed39b58b168e4`.
The integrated lab result validator has SHA-256
`e9029de51794afb41dd32f3950991b280e794bd035f3308503f04ed09bfb70c9`.
The earlier isolated helper revision remains preserved separately. Reporter
source remains `e4ad2ac…`. No measured source drifts during the independent run.

Node 24.12.0, Chromium 156.0.8078.4, Darwin 24.6.0 x64, 16 logical CPUs and
64 GiB RAM. Host load starts 41.47/60.78/44.19 and ends 39.35/60.02/44.02.
These observations describe a contended allocation/ownership environment, not
timing admission. No sensitive timing, calibration of timing cells or admission
attempt is made. All comparisons use one watcher and ordinary membership ops.

Raw evidence report SHA-256:
`3f4b141bb01062588964a7b9ee3dbf378998b487b701b4a176485e157d0b7c25`;
read-only audit/assessment SHA-256:
`72f20dd60a5487498f9fff5ad367a745f959e7d3c9552a3cfdc4d11e41e7ffc1`.
All profile/snapshot maxima are observed and raw artifacts preserved, including
the pre-correction oracle source. No sources or samples are overwritten.

Design is based on existing JavaScript collection contracts and inspected
implementation. Bulk candidate remains frozen at SHA-256
`f7130777350dde04d1e11bc9bf6e4843418a86161d882b669ca2d0913291dd74`;
no product source change is made for this proposal. No timing host is reserved;
the independent discovery conditions are recorded above.

The isolated Promise-policy helper has SHA-256
`fadac2a64008caf2a74abd54cd66feaf906352976f45fa5add8a65c425092dc4`.
Its policy tests run in Node using the existing unit runner with a lab-local
include configuration; no product runner or source is modified.

## Results

### Owner-requested bounded shared-host validation

A short paired Node diagnostic exercised the existing timing probes on the
shared busy host. Product control remained `f713077…`; lab candidate remained
`9fad37793eab029396b9f70069e77aea1ab316cc8f9063ffae899a815c330f2a`.
It used one warmup and seven measured blocks per variant/cell, alternating
order. Each large block held four independent 10,000-item fixtures; small unused
controls held 64. Construction/reset/oracle/abort work stayed outside action
clocks, and existing full-recomputation checks verified each resulting value.

| Action                | Scanning median | Incremental median | Ratio        |
| --------------------- | --------------- | ------------------ | ------------ |
| Count initialization  | 21.953 ms       | 17.832 ms          | 1.23× faster |
| Count single change   | 1.841 ms        | 0.0133 ms          | 138× faster  |
| Count batch           | 15.527 ms       | 0.418 ms           | 37.2× faster |
| Count membership      | 10.713 ms       | 1.780 ms           | 6.02× faster |
| Sum initialization    | 18.206 ms       | 24.851 ms          | 36.5% slower |
| Sum single change     | 7.253 ms        | 0.0119 ms          | 607× faster  |
| Sum batch             | 1.672 ms        | 0.165 ms           | 10.2× faster |
| Sum membership        | 2.063 ms        | 0.894 ms           | 2.31× faster |
| Unused construction   | 5.134 µs        | 5.309 µs           | 3.4% slower  |
| Unused changing write | 1.045 µs        | 0.904 µs           | 13.5% faster |

Unrelated count updates measured 25.393 versus 21.499 µs; unrelated sum updates
measured 7.479 versus 8.293 µs. Sum initialization added approximately 6.65 ms,
versus approximately 7.24 ms saved per relevant sum update in these medians;
that suggests recovery within one relevant update in this sample, not a precise
or independently confirmed break-even guarantee. Wide host/JIT variation and
seven block averages limit small-difference and tail interpretation. The complete
diagnostic, including bulk cells in their own record, finished in 31.115 seconds.

The tested implementation computes correct values and provides a substantial
single-change benefit. This is bounded diagnostic evidence, not completion of all
original timing gates or a product aggregate integration. No numerical criterion
was changed, and no API adoption follows automatically.

### Corrected initialization allocation confirmation (2026-10-27 UTC)

Exactly three profiles per variant/case, one registration per profile, twelve
profiles total. Units are sampled bytes per complete registration. Read-only
reconciliation recomputes all twelve totals from the preserved profile trees and
both envelopes from those totals; they match the coordinator report.

| Case                 | Variant             | n   | Min     | Median  | p95 / max | IQR    |
| -------------------- | ------------------- | --- | ------- | ------- | --------- | ------ |
| Count initialization | Scanning control    | 3   | 184,288 | 208,972 | 212,388   | 28,100 |
| Count initialization | Corrected aggregate | 3   | 3,276   | 9,632   | 11,928    | 8,652  |
| Sum initialization   | Scanning control    | 3   | 198,396 | 198,472 | 221,004   | 22,608 |
| Sum initialization   | Corrected aggregate | 3   | 0       | 540     | 10,072    | 10,072 |

| Case                 | Conservative observed extra-byte envelope | Unchanged criterion | Decision |
| -------------------- | ----------------------------------------- | ------------------- | -------- |
| Count initialization | [−209,112, −172,360]                      | Upper ≤ +1,024      | Pass     |
| Sum initialization   | [−221,004, −188,324]                      | Upper ≤ +1,024      | Pass     |

Both upper bounds establish compliance with the predeclared sampled-allocation
criterion despite variation. The original sum envelope [1,852, 32,308] remains
an observed failure on `8d9e439…`; it is not rewritten or pooled with this set.
The reduction expected from removing the iterator path is now measured for these
fixtures. This is not a universal allocation guarantee for all scanning code.

The sum candidate has sparse sampled sites, including one zero-sample profile;
zero sampled bytes does not mean zero actual allocation. Three-profile extrema
are observed bounds, not confidence intervals; p95 is the maximum. No inspector
charges are subtracted. All per-profile full recomputations/error checks and
unprofiled warmups complete successfully. Existing behavioral and saved-ownership
evidence is reused without rerunning tests or replaying/capturing heaps.

The coordinator takes 2.196 seconds, of which the allocation phase takes 2.149
seconds, inside its 90-second ceiling. This duration is harness cost, not an
initialization timing. No source drift or cleanup errors; twelve profiles only.

**Cumulative effort charge.** Preserve the original 45.914-active-minute discovery
ledger. Charge a conservative **30-minute estimated allowance** for intervening
saved-evidence diagnosis and plan preparation, whose active time was not clocked
as a single run. Charge this approved execution/preparation/reporting a rounded
**five minutes**, beginning 02:02:08 UTC. The cumulative accounting estimate is
**80.914 of the original 90 active minutes**; no budget reset. These preparation
charges are estimates, distinct from the directly measured 2.196-second run.
No unused balance authorizes another sample set or automatic continuation.

### Saved initialization allocation attribution (2026-10-27 UTC)

All evidence in this subsection is reanalysis of the original three profiles
per variant/cell; no resampling. For N=10,000 sum initialization:

| Attribution                                             | Saved sampled bytes/profile  | Interpretation                                                                                 |
| ------------------------------------------------------- | ---------------------------- | ---------------------------------------------------------------------------------------------- |
| Aggregate iteration cluster                             | 205,700–215,100              | Native Map iteration and rebuild-loop self charges; 94.5–99.0% of candidate totals             |
| Other aggregate registration/guard charges              | 1,048–2,060                  | Registration and helper frames; not a separately identified numeric validation cost            |
| Fixture-provided scan iteration                         | 182,720–201,000              | Control array iterator; 94.0–99.5% of control totals; a cost of this scan implementation       |
| Direct `safe` / `Number.isSafeInteger` leaf samples     | 0                            | No direct sampled numeric-validation site; inlining prevents a universal zero-allocation claim |
| Inspector/evaluation boundary outside `allocationApply` | B0: 0–11,176; B1: 528–10,992 | Instrumentation/entry-boundary variation, not safely all attributable to aggregate work        |

The fixture constructs records, reductions/selections, controllers and options
before sampling. Only registration/initial delivery runs inside the initialization
window. Each sum path performs 10,000 contribution checks and 10,000 accumulator
checks on the same safe integer sequence. No numeric error is thrown. The generic
result validator takes its primitive branch and does not probe Promises/thenables
for these values. There is no evidence supporting removal or relaxation of numeric
validation as the correction.

The candidate's temporary traversal is real aggregate work; the control's is
fixture-provided scanning work. Saved stacks cannot identify exact V8 object
types or distinguish every inlined allocation. They do identify the old
iterator-producing traversal as the dominant avoidable path. No single exact
number of excess bytes is assigned to a source expression.

As a diagnostic only, excluding allocations above/outside `allocationApply`
leaves sum extra-byte bounds **+4,164 to +31,728**, so instrumentation-boundary
charges alone cannot explain the observed difference. Preserve the original
complete-action **+1,852 to +32,308** bounds and +1,024 criterion failure. Do not
call either diagnostic partition or the source correction a measured pass.

The corrected lab candidate has SHA-256
`9fad37793eab029396b9f70069e77aea1ab316cc8f9063ffae899a815c330f2a`.
The original candidate is preserved executable beside it and in the measured
source archive at `8d9e439…`. The new loop indexes current keys and looks up each
record directly, creating no Map iterator or per-item iterator-result protocol.
It also follows the contract's current order after reorder, unlike the old Map
walk. The original allocation fixtures had matching initial key/map order, so
this discovered order bug does not invalidate their numeric results.

A failing-before/passing-after initialization/recovery order test and affected
behavior validate the correction. **No allocation profile on the corrected
candidate exists**. Its exact residual overhead and compliance with the +1 KiB
criterion remain a specific measurement gap. No timing benefit is inferred.
Attribution audit SHA-256:
`38a8acae7453c9858e8f5deb5cf95e8dcc9cbd2ce445f741dd2e23dfe39bf99a`.

### Ownership predicate replay (2026-10-27 UTC)

The failures are mechanical and reproducible in saved data:

1. The incarnation predicate checked property names but not a real string key.
   Two compiler-template-like objects with `key → system / Hole` therefore
   matched even when no collection existed. The validated string-key rule excludes
   them; they were not leaked collection items.
2. The record predicate required a `cents` property edge. V8 omits the small-
   integer edge in these captures, though the live record contains that field.
   Existing nonnumeric fields identify the fixture without relying on SMI edges.

The corrected graph has SHA-256
`fafaebd0a15b05d5c330033c72065d1c0caede25f94eab20dcf5306b034a8299`.
Traversal, caller boundary, strong roots and byte classification are unchanged.
All eight old captures pass exact controls on replay: empty calibration has zero
item/record cohorts; B0/B1 at N=10 and N=10,000 have exactly N incarnations and
records, one subscription/context, and respectively zero/one aggregate states.
After abort/drop, all cohorts and all exposed weak references are zero. Recorded
live aggregate root paths remain inside the collection's private wide state.
No new heap capture is taken and no IDs are matched across snapshots.

The previous +672 application-byte estimate at both sizes and separately reported
code/runtime footprint are unchanged. This repairs cohort/lifetime validation
for the **original measured candidate and fixture**, not a generic leak detector
or fresh memory measurement of `9fad377…`. The source correction adds no persistent
owner; that is an inspection finding, not a replacement for recorded scope.
Replay SHA-256:
`5a0904637e826931a3c74fab4b7f8b31c912e49f1969a50750978219435194aa`.
The original failed counts and original classifier source remain preserved.

### Independent discovery evidence (2026-10-27 UTC)

Both independent phases complete: **30 allocation profiles**, then **eight
ownership snapshots**, without timing admission. Preparation/profile/oracle/
teardown phase takes 3.426 seconds; capture/GC/parse phase takes 5.603 seconds;
the owned coordinator takes 9.073 seconds overall. Those are harness wall costs,
not operation latency. Cleanup reports no failure. Each profile uses the fixed
prepared volume in Method, with caller descriptors, oracles and resets outside
sampling. Initial registration and action delivery stay inside their named
boundaries. No counters enter the allocation profiles.

| Cell                           | B0 p50 / p95 / IQR bytes per named action | B1 p50 / p95 / IQR            | Conservative extra-byte envelope | Proposed limit/result         |
| ------------------------------ | ----------------------------------------- | ----------------------------- | -------------------------------- | ----------------------------- |
| Count initialization N=10,000  | 203,600 / 207,744 / 7,604                 | 217,788 / 218,408 / 24,576    | −13,912 to +18,268               | +1,024: inconclusive          |
| Sum initialization N=10,000    | 196,916 / 213,760 / 28,328                | 217,704 / 217,740 / 2,128     | +1,852 to +32,308                | +1,024: observed gate failure |
| Count relevant change N=10,000 | 201,051 / 204,808 / 5,644                 | 813 / 1,214 / 424             | −204,018 to −197,950             | +256: pass                    |
| Sum relevant change N=10,000   | 201,688 / 205,469 / 9,479                 | 551 / 660 / 110               | −204,919 to −195,330             | +256: pass                    |
| Unused ordinary update N=10    | 611.9375 / 665.0625 / 120.8125            | 563.1875 / 604.8125 / 85.0625 | −145.3125 to +60.5625            | +64: pass                     |

Each side/cell has three profiles; nearest-rank p95 equals maximum. Preserve the
sum-initialization observed failure; it is neither passed nor erased because
the intended update path improves. Count initialization's bounds straddle the
limit. No extra profiles are taken. These are sampled extrema, not confidence
intervals or proof of a repeatable intrinsic regression.

Read-only site inspection shows initialization allocation dominated by native
iterator `next` and the candidate's `rebuildAggregate`; attribution moves between
those frames across profiles. B0's selected scanning implementation uses array
iteration, B1 map iteration. These temporary iteration costs belong to these
implementations; not every scanning selection must allocate the same way.
The profile/site data does not isolate a unique avoidable candidate allocation
that establishes the sum gap's cause. No second correction or confirming profile
is permitted in this run. The +1 KiB proposed gate remains unmet.

Work-count tests observe initialization proportional to items, two projections
and one remove/add pair for a relevant single update, and zero projection/reducer
calls for unrelated/reorder-only changes. Batched work is bounded by net changed
operands, with one result publication. Independent source inspection finds no
collection walk on the healthy update path; its full walks are initialization
and declared dirty recovery. Reference sequences cover 200 committed actions at
N=10 and N=100; the large allocation fixtures also check full recomputation
after each sampled action. This distinguishes observed calls from inferred
algorithmic work; neither proves a timing speedup.

#### Ownership evidence and measurement limits

The 128 KiB retained/released probe measures **131,272 bytes**, then zero, and
both weak view/buffer references release. The fixture-root byte estimator reports:

| Held items | B0 application bytes | B1 application bytes | B1 extra | B0/B1 separately classified code/runtime bytes |
| ---------- | -------------------- | -------------------- | -------- | ---------------------------------------------- |
| 10         | 7,792                | 8,464                | +672     | 1,744 / 1,816                                  |
| 10,000     | 2,304,344            | 2,305,016            | +672     | 3,088 / 13,384                                 |

The fixed extra and one live aggregate state at both sizes are consistent with
the no-contribution-cache design; these byte estimates remain qualified by the
classifier limitation below. Do not reclassify persistent code/runtime bytes
as per-item application retention or count them as a framework-owned leak.

After abort/drop, both variants release all six exposed weak cohorts (collection,
keys, sampled record, read callback, run callback, signal). Aggregate state,
subscription and private collection-context classifiers have known live positives
and report zero after drop. These are useful scoped release observations, not
a repaired blanket ownership pass.

The overall classifier fails its broader positive controls: it counts two
incarnation-shaped objects even in the no-collection calibration and after drop,
and detects zero records despite live records. Read-only inspection of the actual
captures explains the mismatch: the two non-item objects have a `key` target
named `system / Hole`, while a live `t0` record exposes `done`/`label` property
edges but omits its small-integer `cents` edge. Requiring the numeric edge makes
the record classifier miss the known positive. This is a measurement defect,
not evidence of retained items. Preserve the failed controls and report full
ownership as inconclusive. No classifier repair, extra snapshot or ownership
investigation follows after consuming the single correction allowance.

All 22 timing cells, break-even calculations and the independent confirmation
remain unmeasured. There is no reserved quiet host. Timing is not inferred from
work counts, allocation reduction or owned-run duration. The missing motivating
timings, unresolved initialization and ownership controls do not warrant spending
the confirmation allowance on a candidate that cannot yet advance.

The tested lab design avoids a contribution cache by
reprojecting the immutable pre-commit record on removal/update. It also requires
an opt-in internal seam for membership record operands and commit-boundary
maintenance, rather than reconstructing deltas in later watcher callbacks.
Method/property timing and full queued-value ownership acceptance remain
unestablished; the observations above do not accept those product requirements.

### Effort and closure ledger

The resumed continuation runs from 00:47:55 to final reporting reconciliation
at 01:13:49.842 UTC: **25 minutes 54.842 seconds**, inside its 70-minute remaining allowance.
Including the earlier estimated 20 minutes of preparation gives approximately
**45.914 active minutes of the one 90-minute budget**. Initial preparation is
an estimate; resumed wall time is recorded directly. No suitable-host waiting
time is charged and no deadline resets. The independent measurement coordinator
accounts for only 9.073 seconds of that work; prototype/test development, harness
adaptation and read-only analysis/reporting account for the rest.

Read-only reconciliation/reporting takes approximately 12.5 minutes, exceeding
its seven-minute planning estimate and using otherwise unspent confirmation time
and the shared remaining balance. The total effort ceiling does not change. No extra measured case,
profile or heap is funded from that allowance. The question closes for specific
evidence limits, **not budget exhaustion**. The useful original isolated policy
source is also reconstructed from its unchanged prefix and verified against its
recorded hash before preserving it alongside the measured sources.

## Correctness

### Corrected initialization order regression (2026-10-27 UTC)

The regression reorders keys from `a,b,c` to `c,a,b` before registering an
aggregate. It asserts projection visits `C,A,B` exactly once, with initial sum 6. It then causes a relevant projection failure, clears it, and commits another
relevant update: dirty recovery must again visit `C,A,B` exactly once and match
full recomputation. The original Map-values loop fails the first order assertion;
the indexed current-key loop passes initialization and recovery. Existing
affected behavior also passes on the corrected revision. This is a confirmed
correctness fix. Removing iterator allocations is an expected performance
improvement requiring the separately proposed confirmation above.

### Approved discovery behavioral gates (2026-10-27 UTC)

The lab copy exercises existing collection behavior and the proposed method,
including exact oracles, net work, rollback, dirty recovery, registration during
delivery, queued prefixes, disposal and actual Promise rejection handling.
Native Chromium gates cover Promise violation routing without cancellation of
rejection events, native abort and queued prefix/rollback behavior. Strict Node
children test initialization, committed maintenance, equality and recovery-seed
Promise violations; no asynchronous value is accepted and later rejection is
handled without global suppression. Test-only declaration checking is reused.

One initial reentrant test expected two scanning callbacks with the same value
`2`. Source and legacy equality behavior establish that the second is correctly
suppressed. The candidate already delivered `1` then `2` as required. Preserve
that failed oracle and correct only its expectation; count this as the **one
targeted correction**, not a product defect or a candidate rewrite. All subsequent
Node/Chromium gates pass before allocation/ownership measurement. No further
correction is made when ownership classification proves inadequate.

Proposed acceptance maps in the contract cover initialization, net committed
updates, insertion/removal, key reuse, rollback, abort/error recovery, disposal,
registration and reentrant prefix delivery. Existing collection tests exercise
the scanning API and are reference behavior, not coverage of the proposed API.
An isolated lab-only Promise policy is tested under the owner's explicit request.
It rejects fulfilled, already rejected and later rejecting Promises synchronously
while a strict Node child survives all settlements; the same child's unobserved
rejection control fails. Own `then` overrides are not read, cross-realm Promises
are observed, arbitrary thenables are not invoked, and discarded fulfillment
payloads cannot create a second derived rejection. No global rejection listener
or event cancellation hides the outcome. These checks use native resources and
the existing Vitest runner; Chromium policy coverage remains a discovery gate.
It is not a complete aggregate implementation or coverage of failure recovery,
reentrant delivery and work counts; those are the first gates of the proposed
discovery. No performance measurement has run. Product tests remain unaffected.

## Threats to validity

The original three-profile initialization comparison has variation exceeding the proposed
1 KiB margin. Sum's observed envelope fails, but neither its source attribution
nor independent repeatability is established. The iterator-heavy scanning control
limits generalization of allocation reductions to other scanning implementations.
Its original ownership-classifier defects are preserved; the later narrow replay
repairs those fixture controls without a new corrected-revision heap measurement.
The bounded allocation confirmation above passes its observed-envelope criterion,
with sparse candidate samples and no claim of statistical confidence. No timing host is
reserved, so intended speedup, ordinary-path latency and break-even remain open.
The candidate is a lab-local source extension, not a shipping-entry acceptance.

Two projections can be expensive for costly item derivations. Pure immutable
reducers can themselves copy large accumulator state. JavaScript floating-point
sums need an explicit arithmetic policy and are not assumed equivalent to an
ordered full fold. Dirty recovery intentionally scans once after failure, and
must not silently drift into routine rescanning. Reentrant committed-prefix
results differ from existing scanning watchers' latest-live-view semantics;
this distinction must remain explicit. None of these costs is measured yet.

## Decision and follow-ups

The owner subsequently authorizes product integration of the recommended design.
The [aggregate contract](../specs/javascript/contracts/incremental-aggregates.md)
owns the supported API. Prior closure/withhold entries below retain their
historical scope; they do not leave another research cycle pending. Product
verification and diagnostics do not assert universal performance or retrospectively
pass unmeasured original timing criteria.

The [owner closure](#owner-closure-incremental-aggregate-research) ends this branch
as inconclusive for API acceptance. The corrected lab-only candidate and its
findings are preserved; product integration and timing acceptance remain
unapproved. The consolidated timing handoff is archival. No automatic
continuation or block on other roadmap work follows. Earlier decisions below
retain their original revision and stage scope.

### Consolidated final-decision recommendation (2026-10-27 UTC)

**Withhold incremental aggregate API acceptance.** Closed correctness,
allocation and ownership evidence is preserved at its stated revisions. Timing
benefit, ordinary/setup costs and initialization break-even remain unresolved;
no attributable regression is established by this handoff. Reopening requires
the single budget amendment above plus a genuinely reserved quiet host, followed
by the bounded timing evidence and an owner decision. No host admission, extra
sample, implementation change, automatic continuation or acceptance commit.
The candidate stays lab-only and outside supported API claims.

### Allocation question closed (2026-10-27 UTC)

**Pass count and sum initialization against the unchanged +1,024-byte discovery
criterion on corrected candidate `9fad377…`; close this allocation question.**
The ordering fix is behaviorally verified and its expected initialization
allocation improvement now has the bounded confirmation above. Preserve all
older outcomes and revisions. Reuse the repaired saved-snapshot ownership results
within their stated scope; no new ownership concern or heap investigation arose.

No extra profiles, candidate redesign, timing attempt or automatic continuation.
Timing benefit, setup/small/unused-operation latency and initialization break-even
remain deferred until a reserved suitable host. The broader discovery still
cannot establish advancement/acceptance without that evidence. No product/API
adoption follows; bulk implementation and its pending timing requirements remain
unchanged.

### Diagnosis outcome (2026-10-27 UTC)

Keep the concrete lab-only indexed initialization/recovery correction and its
behavioral regression. Reuse the repaired cohort/lifetime evidence from saved
captures, keeping all original results tied to `8d9e439…`. Do not drop numeric
validation, subtract inspector charges to pass the original gate, or extend
the ownership classifier beyond its fixture scope.

The discovery remains closed inconclusive: corrected-candidate initialization
allocations still need measured verification, and no reserved-host timings,
break-even or independent confirmation exist. No new measurement campaign or
API adoption occurs in this diagnosis. Bulk candidate `f713077…` and reporter
remain unchanged. Any subsequent measurement requires its own bounded scope;
there is no automatic continuation.

### Discovery closure (2026-10-27 UTC)

**Close inconclusive; do not advance or adopt yet.** Preserve the useful lab
prototype, passing behavior/work evidence, scoped release observations and
relevant/unused-update allocation improvements. Keep the initialization gate
exceedance and ownership-positive failures visible. Missing timing evidence and
unresolved measurement/attribution prevent an advance recommendation; they do
not establish an intrinsic implementation failure or justify deleting the bulk
candidate. No numerical gate changes.

The specific evidence needed to reconsider is initialization allocation
attribution/adequacy, valid broader ownership positive controls, motivating and
ordinary-path timings on a reserved suitable host, then a warranted independent
confirmation. That is not an automatically scheduled continuation. The single
correction is consumed, zero confirmation sets run, and no matrix is expanded.
Any reopening requires an explicit owner priority/budget decision. Product source
and reporter remain unchanged; bulk API acceptance retains its separate timing
handoff, completed allocation evidence and closed retention decision.

The small discovery plan is prepared for owner agreement, with one 90-minute
total effort ceiling, one correction and one bounded confirmation. Approval
would cover only the described lab-local candidate, behavioral/work gates and
measurements; not product API adoption. The isolated Promise policy tests were
explicitly requested and do not start the performance lab. Keep pending bulk
membership and its preserved evidence separate. No unreserved timing attempt,
broad matrix restart or unrelated retention investigation follows this plan.
