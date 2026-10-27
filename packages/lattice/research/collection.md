# Collection cost of bindings

## Question

In a keyed collection, what do Lattice's per-item bindings cost over
hand-written wiring, when items are mounted, updated one at a time, reordered
and torn down? The answer decides whether a cheaper binding setup is worth
comparing, as the [full-flow record](full-flow.md) deferred until a
collection workload shows a meaningful cost.

## Status

### Owner adoption and product integration

**Decided: adopt atomic bulk membership.** The owner authorizes retaining the
bulk implementation as a supported API and integrating incremental aggregates.
`updateMembership` remains on the regular collection entry point, with its
correctness coverage, planner allocation corrections and independent cleanup
fixes. Product integration adds aggregate maintenance through the existing
commit journal. Public reference and authoring guidance cover the supported API.

This decision uses the completed source-scoped correctness, allocation,
ownership and bounded shared-host evidence. It does not retroactively pass
unmeasured original quiet-host/tail gates or transfer prototype timings to the
integrated aggregate revision. Those evidence limits remain preserved below.
The research branch remains closed; no further campaign or admission is pending.

### Owner closure: bulk-membership research

**Decided: close this research branch as inconclusive for API acceptance.** The
owner ends further investigation and timing work. This is a scope/priority
decision, not acceptance or an established performance regression. Existing
collection decisions outside bulk membership retain their recorded scope.

| Finding                                                           | Disposition at closure                                                                                                                                                                                 |
| ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Redundant abort-listener cleanup                                  | Incorporated independently in regular store and collection source, with regression coverage                                                                                                            |
| Direct bulk method and operation-local planner                    | Incorporated in `src/modules/collection/collection.js`, including sized/shared operand buffers, single-insertion fast path and single-removal scalar path; method remains outside supported API claims |
| Correctness, consumers, allocation and framework-owned retention  | Completed evidence preserved at its recorded revisions; no new verification is claimed                                                                                                                 |
| Setup, ordinary/small-operation and bulk latency in Node/Chromium | Unresolved; no final qualified-host timing evidence                                                                                                                                                    |
| Fixture string wrappers, probe calibration and heap ownership     | Measurement corrections preserved; no fixture-driven string normalization added to product code                                                                                                        |

The integrated candidate and independent useful fixes stay intact. No further
admission attempt, measurement, correction or integration is scheduled. The
prepared consolidated handoff is archival, not an outstanding execution task.
Reopening requires a new owner decision with a suitable reserved host and a
bounded comparison of the exact remaining timing gates. Other roadmap work is
not blocked. Original observations, including failed and partial results, remain
preserved below.

### Earlier status entries (historical)

**Consolidated final-decision handoff, 2026-10-27 UTC: recommend withholding bulk
API acceptance.** Correctness/consumer, allocation and ownership evidence stays
closed within its recorded revision scope. Node/Chromium timing requirements
remain unresolved, without a newly attributable regression. No quiet window or
alternate host is currently reserved; no admission is attempted. The
[consolidated timing proposal](incremental-aggregates.md#consolidated-final-timing-handoff-2026-10-27-utc)
records the approved final budget amendment and prepared adapter, preserving all
gates, candidates and controls. Execution awaits confirmed quiet-slot ownership.
No automatic continuation or supported API promotion.

Reported independent allocation confirmation, 2026-10-26: all five representative
bulk action/lifetime allocation gates pass on unchanged `f713077…`; 50 profiles
complete in 5.180 seconds, without admission, timing or heap work. **API
acceptance remains pending only Node/Chromium setup, small-operation and bulk
timings.** Preserve all earlier evidence at its revision. A future timing-only
command reuses this allocation report; no further timing attempt is authorized
without an actually reserved quiet window or another suitable host. The owner
advances incremental aggregate contract design while bulk acceptance is pending.

Reported quiet-window acceptance, 2026-10-26: **withhold supported API acceptance**.
The owner-confirmed command runs against unchanged `f713077…`, but all eleven
host-admission observations fail qualification. The single attempt stops in
60.058 seconds before launching a target process or taking performance samples.
Node/Chromium timings are host-deferred; representative bulk allocations on the
corrected planner remain unmeasured. Reuse the verified allocation correction,
correctness/consumer evidence and closed retention decision. No implementation
regression is established, and the implementation remains intact. No retry,
further investigation, API promotion or conditional acceptance commit follows.

Reported allocation-site correction, 2026-10-26: the planner now uses counted
operand buffers, omits absent insertion operands, and avoids a Set for one
removal. A two-cell diagnostic puts direct append and single removal inside
the unchanged allocation allowance on revision `f713077…`. Preserve predecessor
evidence at its original hash; this is not complete performance acceptance.
The API stays unaccepted. Retention stays closed; timings await an agreed quiet
window with competing workloads paused. No admission attempt or broad rerun.

Reported final remaining-requirements run, 2026-10-26: **withhold API acceptance**.
The unchanged candidate passes batched-append/prepend and representative bulk
allocation gates. Direct-append and single-removal allocation envelopes straddle
their limit and remain unresolved. Host admission fails, so Node/Chromium bulk,
small-operation and setup timings are deferred, not implementation regressions.
Retention stays closed as satisfied; tests are reused. Keep the candidate intact
and unsupported. No automatic expansion, retry or investigation cycle follows.

Current decision, 2026-10-26: **framework-owned retention is satisfied for the
unchanged corrected candidate**. The replacement diagnosis attributes the
remaining template-string delta to supplied values and measurement/GC history,
not unnecessary framework ownership. Preserve the original failed byte result;
this is an ownership decision, not a retrospective numerical pass. Close heap
investigation unless new evidence identifies framework-owned leakage or excess.
The API remains unaccepted pending the remaining allocation and timing gates.
Host contention defers affected timings; it does not reject the implementation.
The approved final run is reported below. Historical conclusions retain their
original revision and measurement scope.

Reported replacement diagnosis, 2026-10-26: replacement strings are inherited
from fixture inputs in both variants; the oracle and GC history affect their
representation. No unnecessary framework owner was found and no runtime
normalization is added. The repaired per-snapshot hidden-owner check passes
on saved and new heaps. Focused replacement action/lifetime allocation gates
pass. The unsampled Node template case still exceeds the original live-byte
gate; timings and unrelated allocation requirements remain deferred. Keep the
single integrated candidate unaccepted, with all previous evidence intact.

Reported corrected-candidate continuation, 2026-10-26: **recommend reject for
this version**, preserving the integrated implementation for the owner's
decision. Corrected calibration succeeds in both runtimes; Chromium lifecycle
graphs pass. Captured Node live application storage exceeds its allowance,
with record-owned string wrappers accounting for the cost. Node hidden-owner
identity evidence is unresolved because capture IDs are not comparable.
Host admission fails; primary timings are inconclusive. The planned live-storage
stop leaves allocation profiles unmeasured. No passing test suite was rerun.

Correction checkpoint, 2026-10-26: the owner preserves the integrated bulk method
and correctness tests, together with the independent regular cleanup fixes.
The calibration fixture now uses an exactly sized byte payload; direct single
insertion avoids attributable general-planner work. No new checks or measurement
have run on this correction. Its remaining performance requirements are
unresolved; the earlier rejection recommendation remains scoped to its frozen
revision and evidence. There is one integrated candidate, not another
experimental runtime or export.

Reported final package candidate, 2026-10-26: **recommend rejection for this
version**. The approved run demonstrates removal/replacement/mixed benefits,
but repeated setup limits and unresolved append/small-operation evidence prevent
acceptance. A calibration-fixture size defect stopped memory/allocation evidence.
The reviewed integrated implementation and coverage stay intact, unsupported
and unaccepted for the owner's final decision. Original evidence is preserved.

Direct-integration follow-up, 2026-10-26: lab-only direct method/module planner
diagnostic completed in 95.401 seconds after an earlier 121.260-second stopped
run. Matched blocks do not repeat a stable near-doubling of construction.
Constructor allocation estimates cannot isolate exact added bytes; C02's
additional sampled allocations fit its allowance. D0 supplies no repeatable
large method/property or larger-code penalty. Keep the direct method/module
planner as the implementation design; no reorder bridge, supported API or
adoption. Earlier unfavorable results, original rejection and proposed retention
clarification remain separate. No automatic continuation.

Reported, 2026-10-25. The owner asked for the lab to run after the framework
guides; this plan was written before any run but not reviewed separately.
Binding setup costs little per item; single-item updates and teardown grow
with the collection. Stage 2 adopted bulk compaction for teardown. Stage 3
compared the shared map with one store per item and is reported with a mixed
result: per-item stores make item updates independent of the collection's
size but cost more memory and teardown than the thresholds allow. The state
shape for collections stays open until the owner decides. Stage 4 evaluated
a keyed collection prototype against agreed thresholds and is reported as
supported: every threshold holds. Decided 2026-10-25: the keyed collection is
accepted for implementation. Stage 5 measured the shipping implementation and
confirmed every threshold, with a confirmation run.

Stage 6 final: **Decided, 2026-10-26 UTC: reject for this version under the
owner's bounded accept-or-discard rule.** The corrected harness stabilizes
probe controls and small append allocations fit, but setup and full-replacement
retention gates are not established, and churn/independent comparisons remain
incomplete at their deadlines. The experimental runtime/export are discarded;
the regular API and independent cleanup fixes remain. Historical results and
measured revisions below are preserved, not replaced by this final decision.

The subsequent matched full-replacement heap diagnosis releases every measured
collection instance graph after caller drop. Remaining code/feedback, shapes
and browser timeline objects have module/runtime roots; whole-heap residue
therefore does not by itself measure collection retention. This does not pass
the original gate or reverse rejection. An explicit shared bulk operation is
now proposed below, without constructor registration or a per-instance closure.

Stage 6 bulk membership: **Reported, 2026-10-26 UTC; current prototype rejected
for adoption by the reduced run's setup-p95 gate.** The owner approved the
20-case amendment and stopping on a clear gate failure. The run stopped after
65.045 seconds, with Q01–Q03 complete in Node; Q03 setup p95 failed. No large
case, small control, Chromium timing, allocation, retention or confirmation
phase was started in that run. This is not evidence against the API shape or
proof of the tail's cause. Shipping code and adoption remain unchanged.
Earlier inconclusive observations and all original reports remain preserved.
The later eight-minute amendment completed both phases in 245.385 seconds.
Its fresh-process setup-tail results are inconsistent (one of three processes
fails the p95 allowance). All three bulk diagnostics show the expected action
benefit, but append teardown is slower in the candidate. No candidate-specific
cause is established; these diagnostics do not change adoption or the original
failed gate. Allocation, retained memory and confirmation remain unmeasured.
The owner subsequently requested an implementation checkpoint. It is authored
at the explicit experimental package entry, with correctness/consumer tests;
the regular entry and measured sources are unchanged. The checkpoint-path
investigation reproduced and fixed redundant once-listener removal. Three
post-fix reproductions passed the abort gates. Admission then stopped at Q02's
setup-p95 gate after two Node cases; allocation, retention and confirmation
were not started, following the agreed rejection stop rule. The checkpoint
remains experimental and unaccepted, with the original sources preserved.
The owner then authorized memory evidence independently of setup admission.
That phase completed all selected cases inside the original sixty-minute
ceiling. Large allocation targets fit, the single-append small allocation gate
fails, and retained-heap acceptance remains unestablished. Setup-only profiles
do not resolve the tail. The regular collection/store received independent
once-listener fixes. None of this adopts bulk membership or starts confirmation.
The subsequent scoped-commit and diagnosis round preserved the measured
checkpoint in Git, corrected attributable single-insertion planning overhead,
and reproduced C02 within its unchanged allocation allowance. Separate
retention calibration still cannot resolve the 16 KiB budget reliably; the
experimental API remains unaccepted and no substantial rerun was started.
The approved cap amendment resolved the small-cell calibration in both engines.
A runner correction preserved an unnecessary extended-calibration attempt and
continued without resetting the original initial-set deadline. The 45-minute
ceiling stopped the set with 12 of 184 timing cells complete. Bulk action
medians show potential, but initial setup/tail concerns, missing comparisons,
confirmation, allocation and retention evidence prevent adoption. Shipping code
is unchanged. Earlier partial runs are retained below.

## Hypothesis

- Each Lattice row binding allocates an internal controller, a combined signal,
  `present`, a subscription and a listener, about 380 bytes per field binding in
  the full-flow lab. Mounting and teardown are expected to cost a few
  microseconds and a few hundred bytes more per item than hand-written wiring.
- A single-item update in Lattice replaces the item and the map of items, then
  every row subscription is considered and reads its own item. Its cost is
  expected to grow with the number of items; hand-written wiring updates one
  row directly.
- Reordering runs the same keyed reconciler in both variants, with one
  selection read in Lattice; it is expected to cost about the same.

## Method

### Consolidated final-decision handoff (2026-10-27 UTC)

The owner approves one bulk timing-only run capped at twelve minutes, aggregate
cumulative effort up to 120 active minutes and a 30-minute consolidated wall
ceiling. The aggregate timing adapter and serial coordinator are prepared; the
exact guarded command is in the linked consolidated plan. Current quiet-slot
confirmation is absent, so no admission, timing or extra measurement occurs.
Closed non-timing evidence and all source revisions remain preserved.

Reuse the existing timing-only runner and closed non-timing evidence on
`f713077…` against preserved control `1cbd0b3…`. The
[single consolidated plan](incremental-aggregates.md#consolidated-final-timing-handoff-2026-10-27-utc)
records exact serial phases, samples, gates, resource needs and the explicit
budget extension request. Bulk retains its 12-minute per-execution ceiling and
independent phase caps. Earlier 60.058-second failed admission and all older
spending remain recorded, not credited back. Because the record has no single
cumulative bulk active-effort balance, the amendment explicitly requests one
bulk timing-only execution allowance instead of silently assuming a fresh one.
Aggregate active spending remains charged cumulatively to its original ledger.
No additional allocation, correctness, consumer or ownership run is planned.

Current reservation: **none**. Finish the handoff and stop without admission.
Recommend withholding bulk support until the bounded Node/Chromium setup,
ordinary/small and representative bulk timing requirements pass. Host
unavailability is unresolved measurement, not an attributable regression.
Keep the implementation intact and outside supported API claims. Only owner
acceptance authorizes documentation/support closeout; inspect the full staged
index before any scoped commit. No automatic run or commit follows this handoff.

### Stage 6: future timing handoff (2026-10-26 UTC)

The runtime source remains `f7130777350dde04d1e11bc9bf6e4843418a86161d882b669ca2d0913291dd74`.
The separate allocation confirmation closes all five corrected-planner allocation
cells. Reuse correctness/consumers, resolved small allocations and closed retention.
The **only remaining acceptance work** is:

| Requirement            | Required future samples in each of Node and Chromium                                                                                   | Unchanged gate                                                                                                                          |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| Bulk timing            | Append/removal/replacement N=C=1,000 and mixed N=1,000 C=100; two fresh sets/runtime, one warmup plus seven fresh actions/variant/cell | Membership median ≤0.50 control beyond both IQRs; mixed median increase ≤max(5%, 0.5 µs); observed tail allowance max(10%, 2 µs)        |
| Small-operation timing | Ten controls, three warmups plus 15 blocks/variant/cell; repeat direct/batched append in a second fresh set/runtime                    | Small bulk median increase ≤max(25%, 2 µs); ordinary median ≤max(5%, 0.5 µs); observed tail allowance max(10%, 2 µs), exploratory tails |
| Setup timing           | Constructor N=10/N=1,000 and subscribed setup N=100; three fresh processes/runtime, five warmups plus 100 blocks/variant/cell          | Median increase ≤max(5%, 0.5 µs), block-average p95 ≤max(10%, 2 µs); all three clean replicas pass; conflict stays inconclusive         |

The owner requests a durable exact command. From the workspace root, **only
after an actually reserved quiet window or selection of a suitable host**:

```sh
node packages/lattice/labs/bulk-membership/quiet-window-run.js --quiet-window-confirmed --timings-only --allocation-evidence packages/lattice/labs/bulk-membership/results/2026-10-26T23-54-58-142Z-bulk-allocations/report.json
```

The explicit allocation report is preserved, source-checked and reused, not
rerun. For another host, transfer the preserved runner, frozen control, report
and pinned fixture sources with their dependencies; hashes must match. The
prepared runner hash after separating modes is
`35537fa85301c6d7cd6ab5a8d47cf3aa81b4394d3dc1522fd46765ae13fd9c0d`.
Earlier measured runner revisions remain archived with their reports.

Retain the 12-minute overall ceiling and existing independent phase caps:
admission 60 s, bulk timing 45 s, small timing 150 s, setup 240 s, report/cleanup
120 s and startup margin 15 s. Omitted allocation time is not transferred.
Retain 20 ms block adequacy, two-second block wall limits, action/preparation
caps, clock uncertainty bounds and all other rules of the approved plan.
Admission still requires eight qualified five-second windows, idle ≥30%,
load/logical CPU ≤0.75 and busy-range ≤15 percentage points. Failed/lost
admission defers timing and preserves evidence without rejecting implementation.
No auto-retry, waiting loop beyond the admission ceiling or threshold change.

No timing attempt is made by this handoff. Bulk API status remains pending;
incremental aggregates proceeds as a separately reviewed proposal at the owner's
explicit direction, not as acceptance of bulk membership.

### Stage 6: independent allocation confirmation amendment (2026-10-26 UTC)

The owner preserves the bulk candidate/evidence, keeps API acceptance pending
and authorizes independent allocation confirmation where its method remains
valid under contention. This sampler measures allocation bytes, not elapsed
time; collected minor/major objects are included and work is normalized per
action. Contention can increase sampling variation or exhaust the ceiling;
neither permits extra profiles or timing claims. No host-admission attempt is
part of this allocation-only command. Keep the same five representative cells,
at most five profiles/variant/cell (50 total), 512-byte sampler, post-profile
oracles, conservative ratio bounds and numerical gates. The separate command
has a 90-second overall ceiling including cleanup/automated reporting. No heaps,
small-profile repeats, passing tests or timings. Runtime source remains pinned
to `f713077…`, control `1cbd0b3…`; the previously measured runner is archived.

Timing resumes only on an actually reserved quiet window or another suitable
host, with the unchanged admission gate before any timing. A timings-only mode
can consume the independent allocation report after checking its source/helper
hashes, all five passing gates, source stability and cleanup. No automatic retry.
The owner explicitly advances contract design for incremental aggregates while
bulk acceptance is pending; this is a sequencing amendment, not bulk adoption.

### Stage 6: corrected planner quiet-window acceptance plan (2026-10-26 UTC)

**Approved and started, 2026-10-26: quiet window confirmed by the owner.** Evaluate the
unchanged collection SHA-256
`f7130777350dde04d1e11bc9bf6e4843418a86161d882b669ca2d0913291dd74`
through the regular package entry against cleanup-fixed control
`1cbd0b38c8a9274c00c5a8f2692319ce84886a659ff69eef5786510fd5b96907`.
The prepared runner SHA-256 is
`c16b7de7d7055b813bf38cdf2c1088bbde22193f38a4358f4674a86bb4fe7ab8`.
It pins the reporter and unchanged action/fixture helpers too, archives measured
sources with unique names, and checks all source hashes again before assessment.
Any drift withholds acceptance. No runtime redesign or edits are part of this run.

Reuse correctness/consumer/JSDoc verification on this exact corrected revision,
the owner's resolution of small-allocation blockers, and the closed retention
decision with unchanged ownership boundaries. Earlier bulk results remain tied
to their measured revisions. Do not rerun small-allocation profiles, passing
test suites, heap captures, retention probes or the deferred larger matrix.

Start only after the owner confirms the quiet window and competing workloads
are paused. Allow roughly three to five minutes of settling before the command;
that is scheduling guidance, not permission to extend admission or the run.
The runner requires an explicit quiet-window confirmation argument and attempts
admission exactly once, before performance sampling. Require eight consecutive
five-second windows: idle at least 30%, one-minute load/logical CPU at most 0.75,
busy-range at most 15 percentage points, and observation intervals between
4.5 and 6.5 seconds. The admission ceiling remains 60 seconds. Failed admission
ends the run with timings deferred and no performance profiles. Lost admission
during timing taints overlapping cells, preserves completed samples and stops
further measurement; neither event rejects the implementation or retries itself.

| Phase, in execution order    | Exact bounded workload                                                                                                                                                                                                                                    | Expected duration                 | Ceiling |
| ---------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------- | ------- |
| Host admission               | One attempt, eight consecutive qualified windows                                                                                                                                                                                                          | 40–60 s                           | 60 s    |
| Bulk timings                 | Node and Chromium, two fresh sets/runtime; append, scattered removal and replacement N=C=1,000; mixed N=1,000 C=100; one checked warmup and seven fresh actions/variant/cell                                                                              | 10–30 s                           | 45 s    |
| Small-operation timings      | Node and Chromium, the existing ten controls; three warmups and 15 measured blocks/variant/cell; a second fresh set repeats only direct/batched append                                                                                                    | 60–130 s                          | 150 s   |
| Setup timings                | Three independent processes/runtime; constructor N=10, constructor N=1,000, subscribed setup N=100; five warmups and 100 measured blocks/variant/cell                                                                                                     | 120–210 s                         | 240 s   |
| Bulk allocation confirmation | Chromium; append/removal/replacement action-only N=C=1,000, replacement complete lifetime N=C=1,000 and mixed complete lifetime N=1,000 C=100; one warmup and at most five profiles/variant/cell, one fresh action/lifetime/profile (50 profiles maximum) | 5–15 s                            | 90 s    |
| Report and cleanup           | Arithmetic assessment, one requirement table, source-drift and cleanup assessment; no further measurements                                                                                                                                                | 60–120 s including reconciliation | 120 s   |

Expected reservation is approximately six to ten minutes after launch; retain
the **12-minute overall ceiling**, with 15 seconds startup margin. Phase limits
remain independent and unspent time is not transferred. Deadlines and profile
counts are ceilings, not completion promises. Preserve unfinished work as
unmeasured; no additional sample, process, cell or automatic continuation.

All previous numerical gates remain. Membership action medians must be at most
half control and improve beyond both IQRs; membership action allocation ratio
must be at most 0.35. Mixed timing uses total-action non-regression: median
increase at most the larger of 5% or 0.5 µs; complete-lifetime allocations at most
1.10 of control, including replacement lifetime. Small bulk median allowance
remains the larger of 25% or 2 µs; ordinary-operation and setup median allowance
the larger of 5% or 0.5 µs. Observed p95 allowances remain the larger of 10% or
2 µs. Fifteen-block p95 is exploratory (effectively the maximum); seven-action
bulk tails are exploratory too. Setup p95 is explicitly p95 of block-average
times, never individual setup latency. Keep process results separate. All three
clean setup replicas must pass; conflicts are insufficient evidence, while two
clean failures of the same gate establish an observed failure without proving
an intrinsic cause. No fourth replica or larger tail campaign.

Keep independent 20 ms accumulation, the two-second wall ceiling per block,
100,000 cheap/single-change action caps and 10,000 others, and the 100,000-live-
item preparation maximum. Preserve matched fixture setup, independent chunk
preparation, public resets, correctness checks and delivery counters outside
action clocks. Constructor inputs/output slots precede its clock; subscribed
setup includes its full existing fixture work and records teardown separately.
Complete actions include input conversion and delivery; complete lifetimes
include construction/action/abort. No fixture or timing boundary changes.

Retain the existing absolute conservative bounds for cap-limited empty/prepend
cells; otherwise minimum-work failures stay unresolved. Keep the same maximum
empty-timer-span error allowance, accounting for each clock pair actually used
in a block (per action for individual action clocks, per chunk for contiguous
clocks). This corrects the old helper's undercount of timer pairs without
changing any numerical limit. Allocation sampling stays 512 bytes including
minor/major collected objects; cross-profile extrema determine the conservative
ratio envelope. Five profiles are maxima: a straddling envelope is unresolved,
not permission to collect more. Clear allocation failures or repeated clean
setup failures end unnecessary measurement and preserve the implementation.

Resource needs: one coordinator and at most one target Node child or one fresh
headless Chromium process tree at a time. Up to seven Node child lifetimes and
eight browser lifetimes run serially; measurement loops are synchronous and CPU
intensive, with browser/helper activity in addition to one main target thread.
Reserve at least four GiB free RAM, approximately one GiB free artifact space
and a 15–20 minute scheduling slot including settling and margin. These are
planning allowances, not newly measured peaks. Pause builds, tests, profilers
and other competing workloads for the measurement period; the runner does not
pause or inspect other sessions. Dependencies and Chromium are already installed.
No installs, network access, external services or loopback server are needed;
browser requests are fulfilled from local files. Browser/owned-child launch
requires host execution access. Preserve existing artifacts in new dated output.

The single prepared local command runs admission, measurements and automated
assessment; it is supplied to the owner outside this record. No command is run
while arranging the window. Recommend accept only when each outstanding gate
is established for this revision, hashes remain stable and owned cleanup succeeds;
otherwise recommend withhold with specific failed, unresolved or host-deferred
requirements. No heap reopening or broad restart. Keep the implementation intact.
If accepted, finish contracts, architecture and public docs/examples, then commit
only the feature's reviewed hunks after inspecting the full staged index. Preserve
other sessions' work and leave none of this work staged while awaiting the window.

### Stage 6: allocation-site correction diagnostic (2026-10-26 UTC)

The owner authorizes inspecting saved allocation sites and implementing/verifying
a concrete improvement, without repeating the broad profile set or taking
timings on an unqualified host. Saved profiles identify single-removal Set
construction and planner-array allocation. Function-level attribution changes
with inlining/native-helper placement, so it cannot determine exact bytes per
source expression. Preserve the original measured source `9643138…` and profiles.

The correction sizes existing operand buffers from captured counts, omits
insertion buffers when there are no insertions, and represents a single removal
with a scalar rather than a Set. Duplicate checks for multiple removals and all
guard/validation/identity/rollback/delivery boundaries remain. Construction and
ordinary operations are unchanged. Corrected collection SHA-256:
`f7130777350dde04d1e11bc9bf6e4843418a86161d882b669ca2d0913291dd74`.

Verification is a two-cell allocation reproduction, not another acceptance
campaign: direct append N=10 and single removal N=100, unchanged control versus
corrected package entry, 1,000 independent prepared actions/profile, one untimed
warmup/variant/cell and at most five profiles/variant/cell (20 profiles total).
Alternate first variant by round. Retain the 512-byte sampler and collected
minor/major objects, equivalent boundaries, post-profile result checks and
100,000-live-item maximum. Report the same +256-byte conservative extrema rule
and saved-site attribution; take no extra profiles if unresolved. Whole diagnostic
ceiling is 60 seconds, including launch/preparation/cleanup and arithmetic
summary. No retention, setup, primary timing, CPU profile or other matrix cells.
Passing checks may be rerun only because their covered source changed.

Timing acceptance remains deferred until an owner-agreed quiet window with
competing workloads paused. Do not attempt host admission, pause other sessions,
or start another acceptance campaign automatically.

### Stage 6: remaining acceptance only (2026-10-26 UTC)

**Approved final amendment, 2026-10-26; run authorized.** The owner approves the
12-minute ceiling, unchanged gates, profile counts as maxima, exploratory
15-block tails, host-deferred timings and reuse without retention/test reruns.
No matrix expansion or further investigation cycle is authorized. Use the unchanged corrected
collection SHA-256
`9643138df205b2215b48726768b8c72b3cf65eacd0f91a06d462b8ded4544b91`
through `@canonic2/lattice/collection`, against preserved cleanup-fixed control
`1cbd0b38c8a9274c00c5a8f2692319ce84886a659ff69eef5786510fd5b96907`.
Freeze both and the harness/report sources before measurement. Keep all original
evidence, budgets and numerical gates. Do not use predecessor timings to certify
this revision. Reuse its passing correctness, consumer and JSDoc checks,
corrected calibration, lifecycle/owner findings, and replacement action/lifetime
allocation profiles. No tests, heaps, retention calibration, constructor
allocation profiles, D0 diagnostics or larger matrix are repeated.

| Remaining requirement               | Smallest comparison                                                                                                                                                          | Decision rule                                                                                                                                                      |
| ----------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Direct/batched append allocations   | Chromium; N=10, one insertion; five profiles/variant/cell, 1,000 independently prepared actions/profile (20 profiles)                                                        | Extra allocation at most 256 bytes/action                                                                                                                          |
| Other small-operation allocations   | Chromium; prepend two at N=10 and remove one at N=100; same sampling (20 profiles)                                                                                           | Extra allocation at most 256 bytes/action                                                                                                                          |
| Bulk allocations                    | Chromium; append and scattered removal, N=C=1,000; five profiles/variant/cell, one fresh action/profile (20 profiles)                                                        | Candidate/control at most 0.35; reuse passing replacement profiles                                                                                                 |
| Mixed complete-lifetime allocations | Chromium; N=1,000, C=100, item updates, membership and full reorder; five profiles/variant, one create/action/abort lifetime/profile (10 profiles)                           | Candidate/control at most 1.10; reuse passing replacement lifetime profiles                                                                                        |
| Representative bulk timing          | Node and Chromium; append, scattered removal and replacement N=C=1,000, mixed N=1,000 C=100; two fresh sets/runtime, one checked warmup and seven fresh actions/variant/cell | Membership median at most 0.50 of control and improvement beyond both IQRs; mixed median within larger of 5% or 0.5 µs, observed tail within larger of 10% or 2 µs |
| Small-operation timing              | Node and Chromium; existing ten small controls, three warmups and 15 measured blocks/variant; repeat only direct/batched append in a second fresh set/runtime                | Small bulk median within larger of 25% or 2 µs; ordinary median within larger of 5% or 0.5 µs; observed tail within larger of 10% or 2 µs                          |
| Setup timing                        | Node and Chromium; three fresh processes/runtime; constructor N=10, constructor N=1,000, subscribed setup N=100; five warmups and 100 measured blocks/variant/cell           | Median within larger of 5% or 0.5 µs; p95 within larger of 10% or 2 µs; all three clean replicas must pass                                                         |

This removes only already-passing replacement allocation cells from the
previous proposal: **70 new profiles**, not 90. Chromium sampling remains at
512 bytes including collected minor/major objects. Alternate first variant
within rounds and retain individual process results. For allocations, the
conservative envelope uses candidate maximum minus control minimum for extra
bytes, or candidate maximum/control minimum for ratios. The reverse extrema
establish a clear failure; overlapping envelopes are inconclusive. Five profiles
are the ceiling, not permission to add samples until a decision appears.

The ten small cells remain empty membership, batched append, prepend two,
ordinary empty batch, ordinary insert, ordinary remove, item patch, reverse,
direct append and bulk remove one. Preserve the existing action definitions.
Construct inputs, reset fixtures, check correctness and count delivery outside
action timings. Complete actions include conversion, commit and delivery.
Lifetime profiles alone include construction and abort. Use equivalent starting
states and work; normalize per action. Generate canonical flat replacement
labels in the fixture, never in the implementation.

Stop each timing block at its first 20 ms of accumulated action time, two seconds
wall time or its action cap: 100,000 only for cheap controls/single-change cells,
10,000 otherwise. Prepare at most 100,000 live items outside timing. Prepend uses
256 fresh fixtures/chunk; other nonempty small cells retain 64-action preparation
chunks and individual action clocks, with resets outside clocks. Setup chunks
are 256 small constructors, four large constructors or eight subscribed
lifetimes; prepare constructor inputs/result slots before clocks, and release,
check and abort afterwards. Full subscribed setup remains inside its clock;
record teardown separately. Keep control/candidate boundaries identical.

Report p95 of **block-average times**, not individual setup latency. Fifteen
blocks make p95 effectively the maximum; seven bulk actions likewise provide
exploratory tail evidence only. Report sample counts, p50, p95, maximum, IQR and
paired differences without promoting those tails to population estimates.
Apply the existing conservative clock-error allowance (twice the maximum empty
timer span). Cap-limited cells remain unresolved unless conservative bounds prove
compliance: prepend candidate p50/p95 upper bounds at most 2 µs and uncertainty
at most 0.1 µs/action; empty bulk upper bounds at most 2 µs, ordinary empty batch
median upper at most 0.5 µs and p95 upper at most 2 µs, with the same uncertainty
limit. No relative speedup claim follows from these absolute bounds.

For setup, conflicting clean replicas mean insufficient acceptance evidence,
not an intrinsic regression. Two clean replicas failing the same gate establish
a repeated observed failure; report attribution separately. No fourth replica.

Run allocations first, independently of timing admission. Then require eight
consecutive five-second host windows within a 60-second admission ceiling:
idle at least 30%, one-minute load/logical CPUs at most 0.75, and busy-range at
most 15 percentage points. Record contention, memory pressure/paging and known
competing work; do not stop other sessions. Monitor during timing at five-second
intervals. Failed admission defers all timings. Lost admission taints overlapping
cells and stops further timings; preserve completed clean cells. Neither case
rejects the implementation or authorizes automatic retries. A later suitable
host runs only deferred cells, not the full matrix.

**Overall ceiling: 12 minutes**, including reporting. Phase ceilings: allocations
90 seconds; host admission 60; bulk timing 45; small timing 150; setup 240;
automated report/cleanup 120; 15 seconds overall startup margin. Ceilings are not
transferable. Prepare the runner and numerical report templates before starting.
Recent replacement profiling took 2.604 seconds for 20 profiles; the remaining
fixture-heavy profiles are estimated at 15–45 seconds. Bulk timing should take
10–30 seconds; small timing 60–130 seconds including untimed preparation; setup
120–210 seconds including preparation and process launches. Setup alone needs
at least 75.6 seconds of accumulated action time. These are estimates, not new
calibration runs. Stop at ceilings and preserve unfinished cells as unmeasured.

Accept only when every outstanding gate is established for the frozen revision.
Distinguish an attributable regression, an observed failure with unresolved
cause, host-deferred timing, and other inconclusive measurement. Host deferral
leaves acceptance pending, rather than invoking the earlier blanket rejection
rule. Keep the implementation intact and unsupported for the final decision.
No new heap investigation is part of this proposal.

### Stage 6: replacement-string diagnosis and ID-free owners (2026-10-26 UTC)

**Authorized focused debugging.** The owner explicitly requests investigation
and repetition of the replacement finding, repair of the hidden-owner check,
correction of attributable implementation retention if found, and only affected
retention/allocation reruns. Timing remains deferred. This is diagnosis of the
reported result, with the existing byte/allocation gates; it is not permission
to restart setup, direct-append timing or the full matrix.

Static inspection finds the exact same `toItemRecord` field-copy/freeze routine
in B0 and B1. Replacement labels originate in the fixture, while its equality
oracle constructs another expected string per item. The diagnosis separates
those sources from the collection operation rather than presuming a planner
allocation or flattening application strings in framework code without cause.

Before further capture, replay the existing heaps with a per-snapshot owner
check. Detect incarnations by actual key/record/subscription-array edges,
subscriptions by fields/read/run/owner edges, collection contexts by their
items/keys/membership/wide slots, and key arrays through their element storage.
Do not require numeric `batch`/`version` edges or exact context display names,
and do not compare IDs across captures. Every live snapshot must positively
detect known instances; every final dropped/churn snapshot must contain zero
strongly reachable dataset owners. IDs serve only to print a root path within
one heap. Keep original false intersections and parser attempts as historical
instrument evidence; no original result is overwritten.

The focused reproduction has **one 480-second coordinator deadline**: retention
240 seconds, affected allocation profiles 90 seconds, offline analysis/report
and cleanup 150 seconds. Counts are maxima; all setup, smoke, capture, analysis,
artifact writes and cleanup count. No extension or automatic sequel. Prepare
the runner/summary template before starting. Reuse the passing affected tests
and calibration on unchanged runtime source. Freeze actual package B1
`9643138df205b2215b48726768b8c72b3cf65eacd0f91a06d462b8ded4544b91`
against unchanged cleanup-fixed B0
`1cbd0b38c8a9274c00c5a8f2692319ce84886a659ff69eef5786510fd5b96907`.
Any attributable runtime correction requires its affected behavioral regression
before final source/path confirmation; do not claim old evidence for changed
code. A fixture/VM representation difference alone is not grounds to alter
the framework's value-copy semantics.

**String cases, N=C=1,000.** Two input modes have identical values: the original
template-generated labels, and labels constructed in the fixture using character
codes, verified as flat by the heap. Nothing normalizes strings in Lattice.
For Node, compare both variants in each mode in two independent fresh-process
sets, alternating first variant. Ten complete matching warmup lifetimes precede
each case. In the first set capture caller inputs held before action; immediate
copied state before any label equality oracle; state after the original oracle;
inputs/snapshots/subscriptions released with collection held; final collection
drop. In the confirmation set capture initialized state before replacement-input
creation, post-oracle released-input state, and final drop. It executes input
creation/action/oracle together in one unsampled task. This preserves the
original replacement interval and avoids confusing pre-action forced GC/aging
from the diagnostic heap with an implementation effect. It has no extra heap
or changed budget. This is **32 Node heaps**. Chromium confirms the flat-mode pair
with the five detailed states (**10 heaps**); earlier Chromium template/lifecycle
evidence keeps its original scope.

The fixture deliberately keeps supplied replacement records until the declared
input-release state. Track those source records weakly, separately from copied
incarnations. Within each heap compare input-label and stored-label targets,
representations, backing/child edges, shared/caller-exclusive/product-owned
storage and strong retaining paths. This reveals whether wrappers predate the
operation, whether the oracle affects them, and whether a framework owner holds
anything beyond the live field value. Do not read/compare labels before the
pre-oracle capture. Functional membership/identity checks remain immediate;
the full value oracle runs afterward.

**Affected hidden-owner confirmation.** The first Node flat-mode commit pair
adds dropped-state captures at 20/100 N=100 churn cycles (**four heaps**).
One fresh Node flat rollback case per variant captures initialized, rolled back,
aborted-held, final drop, churn 20 and churn 100 (**12 heaps**). Total new capture
maximum: **58 heaps**. Keep five fixed GC/task-boundary readings per capture,
scalar/weak metadata, released inspector handles and producer buffers. Per-heap
positive controls require 1,000 live incarnations, 1,002 initial subscriptions,
and the collection context; committed replacement has two subscriptions,
rollback restores 1,002, abort leaves zero, and disposal/churn leaves zero
dataset incarnations/subscriptions/contexts/key arrays. Ambiguity is unresolved,
not a zero-object pass. Earlier heaps are replayed separately, never recounted
as new measurements.

Apply the approved application criterion unchanged: live overhead at most
max(10%, 16 KiB), post-drop at most 16 KiB, zero obsolete owners and no growing
application cohort over 100 cycles. Account for code/shapes/runtime separately;
keep genuine record-owned strings in application storage. Report template and
flat modes separately, including conflicting replicas. An unequal fixture value
representation can explain the prior gate result without retroactively changing
its raw bytes or proving the corrected comparison already passes.

**Only affected allocation confirmations.** Chromium, 512-byte sampling with
minor/major collected allocations, five profiles per variant for replacement
action-only and complete create/replacement/abort lifetime: **20 profiles total**,
one N=C=1,000 lifetime/action per profile, alternating first variant. Canonical
replacement labels use the verified flat fixture; action input conversion and
commit delivery remain inside sampling. All setup/oracles/reference capture
are outside action-only profiles; lifecycle profiles include create/action/abort.
Preserve every sample/site and the original ≤35% B0 action-allocation and ≤110%
B0 lifetime-allocation gates. Five-profile envelopes spanning a gate are
inconclusive, with no extra profiles. Direct append and unrelated allocation
cells are explicitly outside this diagnosis and remain incomplete.

No primary timing, host-admission wait, D0, constructor profile, new browser
runtime, Workbench change or supported API adoption is part of this stage.
The report must distinguish fixture/oracle causes, implementation-owned excess,
repaired owner-check results, allocation outcomes and remaining deferred work.
Keep the candidate, independent cleanup fixes and all original evidence intact.

### Stage 6: proposed corrected-candidate continuation (2026-10-26 UTC)

**Approved, 2026-10-26 UTC.** The owner authorizes the recorded phases without
further pauses and explicitly preserves the passing tests while their covered
source stays unchanged. This continuation evaluates the single candidate, without another
experimental implementation or export. The earlier recommendation and every
unfavorable/partial result remain attached to their measured revisions.

**Approval amendment recorded before measurements:** host contention ends or
invalidates affected primary timing/setup work, but does **not** stop independent
retention and allocation phases where their evidence remains valid. Keep those
phases inside their original fixed ceilings; do not readmit timing automatically
or expand heap/profile counts. Classify contention/instrument limits separately
from attributable regressions, preserve incomplete requirements, and leave the
reviewed implementation intact for the owner's final decision.

#### Source and evidence boundary

B0 remains the cleanup-fixed control
`1cbd0b38c8a9274c00c5a8f2692319ce84886a659ff69eef5786510fd5b96907`.
B1 is the regular `@canonic2/lattice/collection` entry with working-source hash
`9643138df205b2215b48726768b8c72b3cf65eacd0f91a06d462b8ded4544b91`,
checkpointed in public revision `f9aed61b29f0250664d35cb5fe0ab4b283ea00d1`
with unrelated API work kept separate. Pin actual working source/reporter,
fixtures and harness hashes before execution, refuse drift and preserve the
corrected calibration sources recorded below. Do not swap in a lab B1 runtime.
Node uses package resolution; Chromium uses its native package import map.

Reuse affected correctness/consumer results only while their source and fixture
hashes stay unchanged. Preserve earlier operation counts, ownership findings,
allocation attribution and runtime-footprint observations in their exact scopes.
The old whole-heap failure is not erased. Earlier large timings guide the small
confirmation sizes; they do not certify this changed executable revision.
Earlier contended setup, append variation, cap-limited prepend and missing
memory/allocation evidence do not become passing evidence. Do not repeat D0,
CPU attribution, constructor allocation sampling or N=10,000 timing loops.

#### Proposed amendment: one 16-minute coordinator ceiling

This explicitly requests replacing the preceding 13-minute run allowance with
**960 seconds**, not treating the old budget as renewed permission. Numerical
acceptance gates, preparation/call caps and memory ownership criteria are
unchanged. The additional time covers an actual quiet-host admission period,
the previously uncollected profiles and a realistic reporting allowance.
No phase borrows another phase's unused allowance or starts an automatic sequel.

| Order | Phase                                                                       | Fixed ceiling                                                       | Estimated time under admitted conditions |
| ----- | --------------------------------------------------------------------------- | ------------------------------------------------------------------- | ---------------------------------------- |
| 1     | Preflight and host admission                                                | 90 s: up to 30 s JSDoc checking, then at most 60 s host observation | 45–65 s                                  |
| 2     | Corrected calibration, replacement/rollback and churn retention             | 240 s                                                               | 100–180 s                                |
| 3     | Small operations, focused direct append and small-action allocations        | 150 s                                                               | 90–140 s                                 |
| 4     | Small bulk confirmation and outstanding large/lifetime allocations          | 90 s                                                                | 30–60 s                                  |
| 5     | Stable-host setup replicas                                                  | 240 s                                                               | 150–210 s                                |
| 6     | Offline decision tables, retaining-root review, research update and cleanup | 150 s                                                               | 90–140 s                                 |

Execute **admission → retention → direct/small → bulk → setup → report** so setup
noise cannot consume the opportunity to inspect application lifetimes or confirm
the changed insertion path. Phase clocks include their imports, fixture setup,
smoke, profiles/heaps, artifact writes and analysis. Counts below are maxima,
never extensions. Stop owned work at the earliest phase/global ceiling and
mark remaining cells unmeasured. Prepare runner code and empty report templates
before starting the coordinator; do not silently charge report authoring to an
unbounded period afterward.

Estimates use the earlier 1.375-second two-heap calibration capture, the
194-second 24-heap N=10,000 investigation, the matched diagnostic's 53.6-second
setup phase at lower counts and the last run's 163.8-second combined timing
phase before its early stop. These are planning estimates, not claims that an
unexecuted 60-heap or 90-profile campaign has already met its budget.

Preflight adds only `pnpm --dir packages/lattice run typecheck` on the corrected
revision; its earlier pass belonged to the predecessor. Do not rerun the newly
requested passing test suites unless covered source changes. All measurement
groups still get one checked smoke per variant/cell, with counting outside
primary timings. A failed behavioral assertion stops evaluation of that revision.

#### Host admission and contention rule

Record logical CPU count, CPU class, runtime versions, memory, one/five/fifteen-
minute load and aggregate CPU idle time. Admission needs **eight consecutive
five-second windows** inside the 60-second observation ceiling with CPU idle
at least 30% in every window, one-minute load at most 0.75 per logical CPU,
and a range of CPU-busy percentages no greater than 15 percentage points.
Record observed memory pressure/paging and known competing work. No owned
tests/builds/profilers run beside primary timings; never stop another session.
Failure to obtain admission marks all primary timing/setup work
**host-inconclusive**. Under the owner's approval amendment, independent retention
and allocation work continues where valid. Do not wait indefinitely or quietly
raise the admission thresholds.

Continue the same lightweight five-second observations throughout, reporting
load, CPU idle/busy and process conditions before/after each timing/setup cell.
Loss of admission conditions or identified competing heavy work taints the
overlapping cell: preserve it as contention-inconclusive and stop further
timing/setup work, without replacing samples or adding a replica. Contention
does not convert a timing limit into an intrinsic Lattice regression. It does
not excuse a reproducible behavioral/lifetime failure. A quiet admission window
is a practical host-quality condition, not proof that every VM fluctuation is
explained or eliminated.

#### Retention: calibration first, then the exact lifetime graph

In **Node and Chromium**, calibrate once per runtime with the corrected selected
`Uint8Array` and buffer of exactly 131,072 bytes. Capture held/released states
with five fixed GC/task-boundary readings each: **four calibration heaps total**.
Require detection of the attributable payload/backing storage within 16 KiB and
release of both weak identities. Native ArrayBuffer storage counts as application
payload when owned by that selection. Code/templates are not payload; inability
to attribute the buffer is inconclusive, never a zero-byte pass. Calibration
failure stops retention work; do not change payloads or repeat calibration.
Independent timing/allocation phases may still run within their own ceilings
so an instrument limitation cannot masquerade as a product finding.

Then use fresh variant-only processes/realms for each runtime × B0/B1 × commit/
rollback case: **eight cases, N=C=1,000, ten exact warmup lifetimes** each.
Capture warmed empty baseline, initialized collection, full same-key replacement
or rollback with declared caller snapshot holds, owner abort/snapshot release
with collection held, and final owner/caller/collection drop. This is **40 main
heaps**. Use seed-one shuffled removals and original-order replacement records.
Each outcome/variant/runtime then performs **100 N=100 lifetimes**, capturing
dropped state at cycles 20 and 100 against that case's warmed empty baseline:
**16 churn heaps**, at most **60 heaps including calibration** in this phase.
No extra empty-control campaign or collect-until-favorable loop is added.

Keep metadata weak/scalar, release inspector groups and producer buffers, and
perform graph analysis outside product realms. Inspect all tracked product-owned
storage, including arrays, maps, callbacks and private contexts, not only named
record/subscription cohorts. Use baseline identities, corrected rope decoding
and strong retaining paths. Track rollback-discarded records/keys weakly before
throwing; permit explicit caller snapshot holds only in the declared state.
An unknown retaining owner is unresolved evidence, not automatically runtime
footprint. Reject application data retained through code/feedback as well.

Apply the approved criterion unchanged: live application overhead no greater
than max(10%, 16 KiB) over B0; final attributable application retention at most
16 KiB over warmed baseline; no growing application cohort/storage over 100
cycles; **zero obsolete measured keys/records/incarnations/subscriptions/
callbacks/selected-value owners/per-instance contexts after their release**.
Even a tiny obsolete owner fails. Report raw used-heap readings and persistent
module/code/feedback/shape/source/native/probe footprint separately, absolute
and matched B1-minus-B0. Do not invent a code-footprint byte gate or retroactively
pass the original whole-heap result.

#### Direct append and small-operation protection

Use the unchanged B0 and regular B1, subscribed canonical fixtures, N=10.
The primary action covers the complete public call, input conversion/validation,
immutable order creation and commit delivery. Setup, resets, reference capture,
oracles, counters, abort and result checking are outside its timer and measured
separately. Direct append compares `insert` with detached `updateMembership`;
batched append compares existing `batch`/`insert` with `batch`/membership.

In each runtime, one fresh process/set covers all ten prior small cells:
empty bulk, batched single append, direct single append, two-item prepend,
ordinary empty batch/insert/remove/item patch/reverse reorder at N=10, plus
single bulk removal at N=100. **Three warmups and 15 measured blocks per variant**.
A second independent fresh process/set repeats **only direct and batched append**
with those same counts. Alternate first variant by block and offset by set.
Report all sets separately; no pooling or third set. Fifteen-block p95 equals
the maximum and remains exploratory, not population-tail or intrinsic-cause proof.

Ordinary operations retain p50 allowance max(5%, 0.5 µs); small membership p50
max(25%, 2 µs); observed p95/max allowance max(10%, 2 µs). Both append sets must
fit; conflicting sets are inconclusive. Other required small cells must resolve
inside their limits. Stop each block at 20 ms, with a two-wall-second ceiling,
100,000 calls only for cheap/single-change cells and 10,000 otherwise. No cell
gets another calibration loop because it is fast.

**Explicit method amendment for two-item prepend only:** instead of individually
timed restored actions, prepare **256 independent equivalent fixtures** outside
the clock, time the contiguous 256 complete actions, then validate and abort
outside it. This removes per-action clock/reset overhead from a case previously
blocked by duration resolution. Prepare the same chunks for B0/B1; normalize
each block by its actual calls. Stop at 20 ms or the unchanged 10,000-call cap.
If still short, a pass requires B1's p50 **and p95 upper bounds ≤2 µs/action**,
including the entire timed chunk's loop/dispatch overhead plus twice the largest
observed empty-timer span per chunk. Clock uncertainty must be ≤0.1 µs/action.
This conservatively proves the unchanged absolute allowances even against
zero-cost B0. A bound that cannot prove compliance stays unresolved; do not
infer a regression from it or increase the cap. All other nonempty small cells
keep their existing individually timed 64-action/restored-fixture boundary.

The two empty controls retain their approved contiguous-call method and bounds:
empty bulk p50/p95 ≤2 µs; ordinary empty batch p50 ≤0.5 µs and p95 ≤2 µs,
including full block loop/clock overhead and clock uncertainty ≤0.1 µs/call.
No relative speedup claim from cap-limited empty calls. Observe 1,000 empty-timer
pairs once per fresh timing process; no engine tuning or long calibration.

Small-action allocation sampling covers batched append, direct append, two-item
prepend N=10 and single bulk removal N=100: **five profiles per variant/cell,
1,000 independently prepared actions/profile**, Chromium 512-byte sampling with
minor/major collected objects included: **40 profiles maximum**. Setup and
verification are outside action profiles; all prepared live items stay ≤100,000.
Alternate first variant by round and preserve every sample. Require at most
**256 extra sampled bytes/action**; use conservative five-profile envelopes
(max B1 minus min B0) to establish a pass. If the envelopes straddle the gate,
report inconclusive; do not add profiles. Observer costs are reported, not
silently subtracted to improve the gate.

#### Small bulk confirmation on this revision

Node and Chromium, append/scattered removal/full same-key replacement at
**N=C=1,000**, mixed updates/membership/full reorder at **N=1,000/C=100**.
Each of **two independent fresh process sets per runtime** uses one checked
untimed warmup and **seven fresh measured actions/variant/cell**, alternating
first variant. This is 64 B0 actions/runtime including warmups, not another
N=10,000 copying campaign. Keep complete action boundaries and separately report
fixture setup/abort. Independent full recomputation, order/record identities,
and delivery counts are checked outside clocks.

Membership-only p50 must be ≤0.5×B0 in both sets, with gain beyond both IQRs.
Mixed p50 allowance remains max(5%, 0.5 µs); observed p95/max allowance remains
max(10%, 2 µs), with seven-sample tail qualification. Apply twice the maximum
observed empty-timer span to B1 upward and B0 downward; uncertainty that can
flip a result means inconclusive. Preserve the earlier append variability;
do not discard it, pool sets or keep repeating until a narrower IQR appears.

Outstanding allocation confirmations use Chromium's same sampler: five
profiles/variant for action-only append/removal/replacement N=C=1,000, one
prepared action/profile (**30 profiles**); and create/action/abort lifetimes for
replacement N=C=1,000 and mixed N=1,000/C=100, one lifetime/profile (**20 profiles**).
Action-only profiles exclude setup/checks; complete-lifetime profiles include
create/action/abort and exclude oracles/reference capture. Membership action
allocation must be ≤35% of B0; lifetime allocation ≤110% of B0. Conservative
five-profile ratio envelopes must establish compliance; overlap is inconclusive,
not authorization for more profiles. Together with small allocations this is
the original **90-profile maximum**, now explicitly charged to separate phases.

#### Stable-host setup comparison

Three independent fresh Node processes and three separately launched Chromium
processes/profiles, with B0/B1 paired and alternating first within each process.
In each: constructor N=10, constructor N=1,000, subscribed setup N=100;
**five warmups plus 100 measured blocks per variant/cell**. Fixed chunks remain
256 constructors at N=10, four at N=1,000, eight sequential subscribed lifetimes.
Prepare constructor inputs/result slots outside clocks; include full fixture
construction in subscribed setup. Release/check chunks outside clocks; record
abort separately. No forced GC/profiler in primary setup observations.

Each block independently reaches 20 ms or is unresolved, respecting the original
100,000/10,000 call caps, 100,000-live-item preparation limit and two-second
wall ceiling. Report per-process n/p50/p95/max/IQR, counts, duration adequacy,
within-round differences, host conditions and interruptions. p95 is explicitly
**p95 of block-average setup times**. All three admitted, uncontended replicas
must fit p50 max(5%, 0.5 µs), p95 max(10%, 2 µs). Two clean replicas exceeding a
limit establish a repeated observed gate failure and stop unnecessary remaining
measurement; they do not independently establish an intrinsic code cause.
Conflicting clean replicas or any inadequate/contended required replica mean
insufficient acceptance evidence, never majority-vote acceptance or a fourth run.

#### Requirement-by-requirement decision

The final report must separate **pass**, **observed gate failure**, **host/
instrument-inconclusive**, and **unmeasured** for correctness/consumer types,
application calibration/release/live/churn, persistent runtime footprint,
direct/batched append time/allocation, other small controls, setup and large
action/lifetime confirmation. Report footprint as a cost, not a new fabricated
gate. Preserve contention-qualified observations alongside valid earlier evidence.

Recommend **accept** only when every required gate for this frozen revision is
established, including both append sets and all uncontended setup replicas.
Any failed, conflicting, inadequately resolved or unfinished requirement means
**recommend reject for this version**, with the concrete cause classified.
A clear behavioral/ownership or uncontended repeated performance failure stops
unnecessary comparisons; instrument/host limits never become intrinsic-regression
claims. Do not expand the matrix, rerun a failed calibration or consume unused
phase time on additional profiles. Keep the reviewed integrated method, tests
and independent cleanup fixes intact and explicitly unaccepted for the owner's
final decision, even after a rejection recommendation. No publication, discard,
Workbench change or automatic next workload is authorized by this proposal.

### Stage 6: direct single-insertion correction checkpoint (2026-10-26 UTC)

The owner requests a separate implementation checkpoint before further
measurement. Static inspection attributes avoidable work to direct append:
the general order loop checks removal/single-anchor/buckets for every original
key, and allocates empty losses/removal-metadata arrays. A validated request
with exactly one insertion and no removals now copies the order once, then
appends or splices exactly as ordinary insertion does. Removal operands reuse
the shared immutable empty list. Validation, guarded getters, copied record,
new incarnation, plan ownership, undo log and delivery are unchanged. Ordinary
operations and independent cleanup fixes remain intact. No constructor-owned
scratch, additional closure, bridge or experimental implementation is added.

New behavioral cases cover detached append, anchors at first/middle/last keys,
empty collection, copied records, survivor and old-key identities, one delivery,
immediate batch reads and exact rollback. Existing candidate coverage remains.
These cases are authored, not newly executed; earlier passes belong to the
earlier source. Attributable extra work is removed by inspection; its size and
effect on allocations or timings are unmeasured. Bulk membership stays
unaccepted, with setup, small-operation, allocation and application-retention
requirements still unresolved. No further lab or enlarged budget is implied.

Corrected working collection SHA-256:
`9643138df205b2215b48726768b8c72b3cf65eacd0f91a06d462b8ded4544b91`.
The measured predecessor `a34241150895ac77b61cc90094851c8657fd2b0790a7c21ca7ee72c988634c3f`
remains frozen. The correction changes executable planning logic, so its earlier
performance samples cannot certify this revision. The scoped Git checkpoint
isolates membership additions from unrelated API work still in the workspace;
the working-source hash identifies the complete candidate used for review.

### Stage 6: calibration fixture correction checkpoint (2026-10-26 UTC)

The owner requests fixing the fixture separately before further measurement.
Replace the engine-dependent 32,768-slot ordinary array with a `Uint8Array`
whose view and backing buffer both have exactly 131,072 bytes, filled with
the same scalar value. Check both byte lengths before subscribing. The real
subscription remains the sole strong owner after setup returns; weak metadata
tracks both the view and backing buffer, and calibration requires both to
release after abort. The graph includes backing-store/native storage as
application-owned payload rather than classifying it as persistent runtime
footprint. If that storage cannot be attributed, calibration must remain
unresolved; no threshold or graph subtraction is changed.

The original failed calibration, its hashes and two heaps remain preserved.
The old probe/runner are reconstructed byte-for-byte against their recorded
hashes and retained locally alongside the corrected harness checkpoint.
Only the research description and source identities enter Git; lab-specific
runners and artifacts remain ignored. This is a fixture correction, not a
rerun or retroactive pass. The prior once-only stop remains respected; a new
comparison needs its own bounded scope. No checks or measurements are started.

Corrected probe SHA-256:
`31a297a3ca073c4fd640caf0c9bc812e0832d599fb83d63e815a25a75a434935`;
corrected coordinator:
`3df11f379e60a3a419820fdf032ea6edf7946a570eae0c6d457f071fc1ec633e`.
Measured predecessors are respectively
`a0136ea57bfe60ed18dcf684f363aba9c07a3d138b365547b54ccc1a8111f523`
and `059fc07cb83b7e5ad34080de6ef44b1d986941ec18938013ae160ab2f0b53128`.
The coordinator deliberately still refuses an unmatched collection revision;
a future approved comparison must freeze and name its corrected source first.

### Variants

Both variants mount the example's `<acme-filter-field>` as rows, one per item,
showing the item's label and disabled when the item is done, and run the same
lab-written keyed reconciler. The reconciler validates keys first, removes rows
whose keys are gone, creates rows for new keys and moves rows into order with
`moveBefore`, as [DOM updates](../specs/javascript/contracts/dom-updates.md#keyed-collections)
requires. Each row has its own lifetime: an `AbortController` aborted when the
row is removed or the collection is torn down.

| Variant         | State                                                                                                      | Wiring                                                                                                                                                                                      |
| --------------- | ---------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0. Hand-written | A map of items and an array of ids                                                                         | Actions update the map and assign the row's inputs directly; one listener per row with the row's signal                                                                                     |
| 1. Lattice      | A tasks capability on `createStore`: `order`, an array of ids, and `byId`, items by id, replaced immutably | A collection binding watches `order` and reconciles; a row binding per item, in the bindings contract's shape, watches its item's selection and calls the label action from the row's event |

No new Lattice API is introduced; the capability, bindings and reconciler are
lab code on the store's public API.

### Workloads

| Workload           | Sequence                                                                             |
| ------------------ | ------------------------------------------------------------------------------------ |
| Mount              | Create the state, the rows and their wiring for N items, connected                   |
| Single-item update | Change one item's label, alternating between two values, on items chosen with seed 1 |
| Reorder            | Reverse the order of all items                                                       |
| Teardown           | Abort the collection's lifetime: every row lifetime ends and every row is removed    |

Sizes: 10, 100 and 1,000 items.

### Measures

| Measure                | Evidence                                                                                                                                                   |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Time per operation     | Observed: `performance.now()` around mount, teardown and reorder per item, and around 1,000 single-item updates per update, in cross-origin isolated pages |
| Allocations per update | Profiler: steady-state bytes per single-item update after 20,000 warmup updates, sampled at 32 bytes                                                       |
| Memory per item        | Profiler: JavaScript heap after garbage collection, mounted minus empty, divided by N                                                                      |
| Work counts            | Observed: selection reads, row input assignments, control and text writes, moves, and rows created and removed per operation                               |
| Cleanup                | Observed: observers disconnected, and DOM listener and node counts back to their baseline after teardown                                                   |

### Runs and reading

Each sample is a fresh browser context loading one variant, in an order
shuffled with seed 1. Timing: 15 pages per variant and size. Allocations and
memory: 10. Counted: 3. Each timed operation has 5 warmup repetitions and 10
measured ones.

Results are read as in the full-flow lab: a meaningful difference exceeds 5%
of the control and both interquartile ranges. A cheaper binding setup is worth
a comparison when mount, teardown or memory per item is meaningfully higher in
Lattice by an amount that matters at the measured sizes; the record states the
absolute figures so that judgment can be made.

### Correctness checks

Every counted page checks, in both variants: rows match the items after mount;
an update changes exactly one row; a reorder moves rows without creating or
removing any; after teardown no row remains, observers are disconnected and
listener and node counts return to their baseline.

### Stage 2: bulk compaction (2026-10-25, before any stage 2 run)

**Question.** Does deferring the removal of deactivated subscription records
remove the quadratic teardown without costing retained memory or delivery?

**Variants.** Variant 1 is the Lattice collection with the current store.
Variant 2 serves a lab copy of the store in its place, changed in one place:

- Deactivation stays immediate: the record is marked inactive and its `read`,
  `equals`, `run` and last value are released at once, during a delivery too.
- Only removing inactive records from the array is deferred: outside a
  delivery, one compaction pass runs when inactive records exceed half the
  array, and at the end of a delivery under the same condition. No timer or
  scheduling is introduced.

**Workloads.** Teardown of 10, 100 and 1,000 items; heavy churn, mounting
1,000 items and removing 900 one at a time, then timing single-item updates on
the remaining 100 and measuring the heap; and 20 cycles of mounting and
removing 100 items, timed and followed by a heap measurement.

**Reading.** Variant 2 is adopted when teardown at 1,000 items is meaningfully
faster, no other measure is meaningfully worse, retained memory after churn
and cycles is no larger by more than the inactive records allowed to wait,
and every correctness check passes, including that no callback of a removed
row runs. Adoption means implementing the change in the store with its unit
tests passing and a test that deactivation releases callbacks immediately.

### Stage 3: collection state shape (2026-10-25, before any stage 3 run)

**Question.** Can the existing API meet the collection requirements in the
[state management contract](../specs/javascript/contracts/state-management.md#collections)
with one store per item, and at what cost compared with the shared map?

**Variants.** Both use the store as committed, with bulk compaction, and the
same rows, reconciler and per-row lifetimes as stages 1 and 2. Keyed
subscriptions and windowing are out of scope.

| Variant           | Shape                                                                                                                                                                                                                                                                                                                                                                                                                  |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1. Shared map     | One store holding `order` and `byId`, replaced immutably. A collection-wide selection, the number of open items, declares `order` and `byId`                                                                                                                                                                                                                                                                           |
| 3. Store per item | The capability keeps `order` in a membership store and creates one store per item holding `label` and `done`. `item(id)` returns a stable read-only view of that item's store, `{ get, watch }`, or `undefined` for an id not in the collection; label and done selections are shared by every item. The open count is computed over the items by a summary binding that watches the membership and each item's `done` |

Both variants define the same rules:

- Writes go through named actions only: `setLabel`, `setDone`, `setLabels` for
  several items, `add`, `remove` and `reverse`.
- An action naming an id not in the collection throws a `TypeError` and
  changes nothing.
- Removing an item ends its row's bindings, by aborting the row's lifetime,
  before the item's store is released.
- A bulk update of several items is one commit with the shared map and one
  commit per item with stores per item. With stores per item, subscribers can
  observe a partially applied bulk update between those commits; this is
  documented, not prevented, and no cross-store transaction is added.

**Workloads**, at 10, 100 and 1,000 items: mount; single-item label update;
single-item done toggle, which changes the open count; bulk label update of
10 items and of every item; insertion of one item at the end; removal of one
item; reorder by reversal; teardown.

**Measures.** CPU time per operation, as `performance.now()` around the
synchronous operation in isolated pages, which includes the DOM writes and
excludes rendering, identical by construction in both variants; steady-state
bytes allocated per single-item update, done toggle and bulk update of every
item; JavaScript heap per mounted item; and counted DOM effects, selection
reads and open-count derivations per operation. Pages and repetitions as in
stage 1.

**Correctness checks**, on every counted page, in both variants: identical DOM
effects per operation; keyed identity, every remaining row keeping its element
across insertion, removal and reorder; the open count correct after every
operation; teardown leaving no row, observer, listener or node; an action on a
removed id throwing a `TypeError`; and no callback of a removed row running.

**Decision thresholds**, set before running. Store per item is supported as
the collection shape when every condition holds:

1. A single-item label update at 1,000 items is meaningfully faster than the
   shared map, and at 10 items no more than 25% slower.
2. A bulk update of every item is no more than twice the shared map's time at
   any size.
3. Mount, insertion, removal, reorder and teardown are no more than 25% slower
   at any size.
4. Memory per item is no more than 50% higher at 100 and 1,000 items.
5. A done toggle, which changes the collection-wide open count, is no more than
   25% slower at any size.
6. Every correctness check passes.

If conditions 1 and 6 hold and others fail, the result is mixed: the record
states which costs store per item adds, and the contract stays open with that
evidence. If condition 1 or 6 fails, store per item is not supported. In every
case the contract stays open until the owner decides.

### Stage 3 follow-up: memory attribution and bulk done updates (2026-10-25, before any follow-up run)

The owner asked, after stage 3, to attribute store per item's extra memory
and to measure bulk updates of `done` before accepting those costs or
considering keyed subscriptions.

**Bulk done updates.** Both capabilities gain `setDoneMany(ids, done)`: one
commit with the shared map, one commit per item with stores per item, under
the same rules as `setLabels`. Workloads at 10, 100 and 1,000 items: a bulk
done update of 10 items and of every item. Before each measured set, an
untimed call sets the chosen items to not done; measured calls then alternate
done and not done, so every call changes every chosen item. Measures: CPU time
per call (15 pages, 5 warmup and 10 measured rounds), steady-state bytes per
bulk update of every item (10 pages), and counted DOM effects, open-count
derivations and summary text writes per call, with the rows and open count
checked against the bench's model after each (3 pages). Expected: stores per
item derive the open count once per changed item, so a bulk update of every
item costs work proportional to N², and the summary text is written once per
commit, showing the partly applied update. Reading: the result is reported
against stage 3's condition 2, at most twice the shared map's time at any
size, and the record states whether that condition still holds when the bulk
update changes the field the collection-wide value reads.

**Memory attribution.** Diagnostic variants, never adopted: 1n and 3n are
variants 1 and 3 without the summary binding. A bare-store cell creates N
stores holding `label` and `done`, with no bindings, and keeps them. JavaScript
heap per item after garbage collection, mounted minus empty, for 1, 1n, 3, 3n
and bare stores at 10, 100 and 1,000 items, 10 pages each. Variant 3 minus 3n,
less the shared map's single summary subscription (1 minus 1n), gives the
summary binding's per-item cost; 3n minus 1n gives the cost of the item
stores in the collection; the bare-store cell gives a store's own size.

### Stage 4: keyed collection prototype (2026-10-25, before any stage 4 run)

**Question.** Does the keyed collection proposal, since accepted as the
[collections contract](../specs/javascript/contracts/collections.md), as amended by the
owner's decisions of 2026-10-25, meet the collection
requirements within the agreed thresholds?

**Authorization.** The owner accepted the separate collection capability as
the candidate, decided the proposal's open questions, required transactional
removal, order-dependent invalidation of collection-wide values and
incarnation semantics for reused keys, agreed the thresholds below, and
authorized running this comparison without further approval. Nothing ships
from it.

**Variants.** Variant 4 is a lab prototype of `createCollection`, written to
the proposal, with a tasks capability on it: actions `setLabel`, `setDone`,
`setLabels`, `setDoneMany` (the bulk actions in one `batch`), `add`, `remove`
and `reverse`; a membership binding that replaces the rows of removed and
reinserted keys and reconciles with the shared reconciler; row bindings with
one `watchItem` per input; and the summary bound with `watch` on the open
count, `items: ['done']`. Variant 1, the shared map, is the baseline; variant
3, store per item, is a reference. All use the committed store and the same
rows, reconciler and per-row lifetimes.

**Prototype tests**, run before any measurement, in Node with the package's
test runner, covering the proposal: snapshots and identity of keys and
records; function patches and rejected reentrant writes; insertion with and
without `before`; reorder validation; the three-phase delivery order; new
subscriptions skipping the commit in progress; collection-wide values
re-evaluated on reorder and suppressed by `equals`, and not evaluated for a
commit that changes none of their fields; a bulk action in one batch notifying
each subscription at most once; transactional removal, including rollback by
the outer batch and by nested savepoints, restored subscriptions, and a
subscription aborted during a rolled-back batch staying ended; key reuse
inside a batch, across commits and during delivery, with queued
notifications for the old incarnation dropped; and the delivery error policy.

**Workloads**, at 10, 100 and 1,000 items: mount; label update; done toggle;
bulk label and bulk done updates of 10 items and of every item; insertion;
removal; reorder; teardown.

**Measures.** CPU time per operation as in stage 3; steady-state bytes per
label update, done toggle and bulk label and bulk done update of every item;
JavaScript heap per mounted item; for item subscription overhead, heap per
item of a bare collection against a bare shared-map store with the same items,
each with no subscriptions, so that the mounted heap minus the bare heap gives
what the bindings add per row; and counted DOM effects, `read`s, open-count
derivations and summary text writes per operation. Pages and repetitions as in
stage 3.

**Correctness**, on every counted page: row DOM effects identical to the
shared map; at most one open-count derivation and summary write per action,
none for a label update, and no summary write when the count is unchanged;
rows and open count matching the bench's model after each operation; keyed
identity across updates, insertion, removal and reorder; a removed key
reinserted in one action getting a new row while the other rows keep theirs;
actions on a missing key throwing a `TypeError` with no effect; and teardown
leaving no row, observer, listener or node.

**Thresholds**, agreed by the owner before the run. The proposal is supported
when every condition holds:

1. A label update at 1,000 items is meaningfully faster than the shared map
   and within 2× of store per item.
2. A bulk update of every item, of `label` or `done`, is no slower than the
   shared map by more than 25% at any size, with at most one collection-wide
   derivation and at most one summary write per action.
3. Mount, insertion, removal, reorder and teardown are no more than 25%
   slower than the shared map at any size.
4. Heap per item is no more than 25% higher than the shared map at 100 and
   1,000 items.
5. Every correctness check passes, including the prototype tests.

Otherwise the record states which conditions fail, and the collections
question stays open.

**Amendment (2026-10-25, after the smoke run, before any stage 4 run set).**
The first smoke run passed every check but showed the prototype allocating far
more than its design requires: about 2.5 KB per done toggle and 470 bytes per
item in a bulk update at 100 items. A sampling profile found three causes in
the prototype, not in the proposal:

- iterating the frozen keys array with `for…of` in the summary's `read`
  allocated an iterator result per element in this Chromium, while the other
  variants iterate arrays that are not frozen;
- every commit allocated a change object, wrapper arrays, a list of changed
  fields and a queue record;
- batches logged each write as a tuple array, tracked changed items in a
  `Map` and compared every field with `for…in`.

Before any measured run, the prototype's internals were rewritten without
changing behavior: commits are queued in parallel arrays as in the store, the
undo log is flat, an item's base record is kept on its incarnation, and
delivery compares declared fields between the records before and after each
commit. Variant 4's summary iterates the keys with an indexed loop, with a
comment saying why. The prototype tests, extended with a savepoint case, pass.
The `for…of` cost is a finding for the proposal: freezing the keys array
makes the natural loop over it allocate in this engine.

### Stage 5: shipping path (2026-10-25, before any stage 5 run)

**Question.** Does the shipping keyed collection, implemented from the
[collections contract](../specs/javascript/contracts/collections.md), meet the stage 4
thresholds on the shipping path, and what does it allocate and collect?

**Authorization.** The owner accepted the proposal for implementation on
2026-10-25 and asked for the comparison to be repeated through the shipping
path, including a confirmation run, without further approval.

**Variants.** Variant 5 is the tasks capability and bindings of variant 4,
unchanged except that it imports `createCollection` from
`@canonic2/lattice/collection`, served unchanged from the package's source
through the page's import map. Variant 1, the shared map, is the baseline;
variant 3, store per item, is needed for condition 1; variant 4, the lab
prototype, is a reference.

**Workloads, measures and correctness** as stage 4, for variants 1, 3, 4 and 5
in timing, 1, 4 and 5 in counted and memory sets. The counted checks of stage 4
apply to variant 5, including the summary limits and key reuse.

**Garbage collection.** For variants 1 and 5 at each size, 10 pages each: after
a warmup, trace Chromium's `MinorGC` and `MajorGC` events while running 20,000
label updates, 20,000 done toggles and a bulk done update of every item
repeated 20,000 ÷ N times (at least 20), each separately, and report the
number of collections and their total duration per 1,000 operations next to
the run's own CPU time and allocated bytes.

**Confirmation.** After the first timing set, a second timing set of variants
1 and 5 alone, 15 pages per size, under comparable conditions.

**Reading.** The shipping path confirms the stage 4 result when the five
thresholds hold for variant 5 in the first timing set and in the
confirmation, and in the memory and counted sets. Allocation and collection
are reported against the prototype and the shared map with no threshold: the
owner accepted bulk allocation as a cost of this design. If a threshold fails,
the prototype figures do not carry over to the product, the record says which
measure failed, and the owner decides.

### Stage 6: bulk membership (2026-10-25, plan awaiting approval)

**Question and scope.** Can one explicit bulk membership operation eliminate
repeated whole-order copies while leaving small existing operations cheap?
This continues the collection question with the current shipping collection
as control, rather than carrying forward stage 5's performance conclusions.
The proposed behavior is owned by the
[bulk membership proposal](../specs/javascript/contracts/collections.md#atomic-bulk-membership).
Its method name, anchor restriction and thresholds below are all proposed.

**Owner amendment, 2026-10-25, recorded before measurements.** The owner
approved `updateMembership({ remove, insert })` for evaluation, including
surviving-key anchors, stable insertion order and unchanged existing batch-read
semantics. Apply the 2× speedup and 65% allocation-reduction gates only to
membership-only bulk workloads. For mixed actions containing item updates and
a full reorder, require no meaningful total-action regression: use the
recorded non-bulk p50 allowance (the larger of 5% or 0.5 µs) and the recorded
p95 allowance (the larger of 10% or 2 µs). Report mixed-action improvement
without requiring unrelated work to be halved. All correctness, identity,
rollback, cleanup, small-operation, lifetime-allocation and retention gates
remain. The run budget is a ceiling. Complete smoke and calibration first;
stop early on correctness failure or if calibration cannot resolve the
thresholds. No public API adoption or shipping implementation is authorized.
This amendment supersedes the mixed-workload speedup/allocation requirement
and the original paragraph treating every workload as a speedup gate below;
the original proposal is retained as the pre-approval plan.

**Hypothesis.** Source inspection shows `insert` spreads the complete keys
array and `remove` filters it on every call, including inside `batch`.
For C membership changes over N items this can copy O(NC + C²) keys.
A supplied atomic change set can build one next order in O(N + C), plus item
record copying. This is an algorithmic prediction, not a timing or allocation
measurement. Lazy publication for existing operations is rejected because
reads inside a batch must observe each pending write; a linked mutable order
would also impose new bookkeeping on every collection.

**Variants.**

| Variant             | Implementation and supported scope                                                                                                                                                                                                                                             |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| B0, control         | Current `@canonic2/lattice/collection`, unchanged, using repeated `remove` and `insert` inside one `batch`; existing item updates and reorder stay in the same batch where requested                                                                                           |
| B1, candidate       | Local copy of the same module with only `updateMembership` added, following the proposal: prevalidated removal set and insertion buckets, one final frozen keys array and existing undo/commit/delivery paths; complete proposed semantics, no alternate subscription strategy |
| D0, diagnostic only | Build the same final plain keys array from the normalized changes without a collection, records, rollback or subscriptions; estimates the order-building floor and cannot be adopted                                                                                           |

B0 and B1 import through equivalent explicit module entry points, including
their real initialization. B0 source stays read-only. B1 and runners stay
local and ignored. No production instrumentation or duplicate shipping backend.
First inspect any retained local runner for reuse; previous runners do not
establish these workloads or correctness. No reusable fixtures directory exists
at planning time; this stage's deterministic generator remains local.

**Inputs.** Use keys `t0` through `t(N-1)` and records
`{ label: 'Task i', done: i % 3 === 0 }`. A seeded 32-bit xorshift generator
(seed 1; shifts 13, 17, 5) selects distinct scattered indices by Fisher–Yates
shuffle. Use N = 100, 1,000 and 10,000. For each N use C = 1, 10,
ceil(N/10) and N, deduplicating identical sizes. Include an extra N = 10,
C = 0, 1 and 2 matrix to expose small-operation overhead. Every variant uses
the identical canonical fixture and expected output.

| Workload            | One logical action                                                                                                                                                                                           |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Append              | Insert C fresh keys at the end                                                                                                                                                                               |
| Prepend             | Insert C fresh keys before the first original key, preserving supplied insertion order                                                                                                                       |
| Scattered insertion | Insert C fresh keys distributed over original anchors chosen by the seeded sequence, with multiple entries per anchor and append entries                                                                     |
| Scattered removal   | Remove C distinct original keys in seeded order, including complete removal at C = N                                                                                                                         |
| Replacement         | Remove C original keys and insert new records under those same keys in their original positions; anchor each run before its next surviving key, or append when none survives                                 |
| Mixed batch         | Remove C original keys and insert C fresh keys before surviving anchors or at the end, update `done` and `label` on min(C, N-C) survivors, then reverse the complete resulting order, all in one outer batch |

For replacement, insertion tuples are in original order; removal order remains
seeded. For mixed batches, field updates precede membership, and reorder follows
it. The B0 adapter performs all supplied removals then insertions in their
supplied orders. B1 uses one bulk call inside the same outer batch. Also record
standalone single `insert` and `remove` on B0/B1, plus empty change sets and
item-only updates, to detect costs imposed on paths that do not use the API.

Run two subscription modes: none, and a representative mode with one item
watcher per original item plus one membership watcher and one collection-wide
membership-count selection (`fields: []`, reading `size`). Item watchers read
`label` and `done`. Register replacement watchers only after the action; they
must receive only their initial delivery. DOM and presentation are excluded
because this decision concerns the pure collection. Setup is timed separately.

**Expected results and correctness.** A straightforward array/map oracle
applies removals and sequential insertions outside timed regions. Compare exact
order, item values and survivor identities after every action. Compare
membership metadata and all delivery counts with B0, including initial calls
separately; each affected subscription is evaluated at most once per commit.
Mixed actions must expose no partial result to callbacks. Replacement must
deliver membership metadata even when key strings and order match.

Every sample also runs a separate untimed semantic sequence against each
variant: retained frozen arrays across successive reads/writes; reads inside
ordinary batches between existing operations; bulk calls before/after ordinary
writes; nested successful savepoints and caught inner failures; invalid input
and throwing getters after an earlier outer write; rollback after replacement;
insert/remove transient items; abort during a rolled-back removal; cleanup of
removed subscriptions and live signal listeners; new watchers skipping earlier
queued commits; reentrant membership and item writes; initial and delivery
errors with the existing reporter rules. B1 additionally rejects removed or
new insertion anchors without partial mutation. Weak references and forced GC
in separate retention samples check released keys, records and callback
captures after both commit and rollback; retained fixtures must not keep them
alive accidentally. A failed semantic comparison rejects the candidate.

**Timing boundaries.** Primary metric: total synchronous elapsed time from
logical-action entry through final commit delivery, including conversion from
the canonical workload into API input and input validation. Collection setup,
subscriber setup, fixture generation, oracle evaluation and teardown are
outside that boundary and reported separately. Also time the API call alone
with prebuilt input as a diagnostic; it cannot replace total-action time.
Use monotonic `performance.now()`. No profiler, counters or forced GC runs
inside timing samples. Fresh fixtures prevent growth across repetitions;
keep construction outside the timer but report the complete create/action/
abort lifetime as its own measurement. Teardown aborts every owned signal and
drops the collection; there is no public collection disposal method.

**Calibration and repetitions.** Run only after agreement. First smoke every
workload group with B0/B1, one sample per size, with full semantic comparisons.
Then calibrate timer overhead and repeat counts using the N = 10 and 100
small-operation cells. Accumulate separately timed chunks of independent
prepared collections until their summed action time spans at least 20 ms per
block; reset fixtures outside the timed chunks. Cap a block at 10,000 actions
and simultaneously prepared live data at 100,000 items. If the cap prevents
reliable resolution, label the
cell unresolved instead of stretching the budget. Fix repetition counts before
the main set. Each cell takes five warmup blocks and 15 measured blocks;
alternate B0/B1 order using seed 1. A second independent set repeats decisive
cells: all workloads at N = 1,000/10,000 with C = ceil(N/10) or N, and all
small/non-bulk controls. Report n, p50, p95, max and IQR, including absolute
differences; p95 with 15 samples is exploratory. Compare per-cell paired ratios
and both run sets, without pooling away losing cells.

**Allocations and retained memory.** In separate Chromium CDP sessions use
HeapProfiler allocation sampling at 512 bytes with collected objects included.
Measure action-only and total create/action/abort lifetime in separate samples;
report estimated allocated bytes/action, allocation-site attribution and sample
counts, never call sampled bytes exact total allocations. Five independent
profiles per cell run at least 1,000 actions or 1,000,000 supplied changes,
whichever comes first, with an identically prepared empty-loop control.
For action-only profiling, start sampling after preparing each bounded chunk
and stop before teardown; combine attributed samples across chunks. Lifetime
profiles include those phases. D0 runs only in the no-subscription timing and
allocation cells at N = 1,000/10,000 and C at least ceil(N/10); it has no
contract or retained-subscription result.
Do not subtract profiler overhead from timing results. If sampling cannot
resolve a comparison, mark it inconclusive and propose a method amendment
before further profiling. This does not authorize an unplanned profiling run.

For retained heap, collect five independent samples per cell after explicit GC:
empty realm; initialized collection and subscribers; completed action with live
collection; and aborted/dropped collection after commit or rollback. Drop
temporary inputs, oracle results, callback histories and old snapshots before
the post-action snapshot. Also measure caller-retained original keys separately
so required snapshot retention is visible rather than mistaken for a leak.
Report baseline-adjusted live bytes, bytes/item and post-teardown growth. Record
forced-GC convergence; failure to converge is unresolved, not zero retention.

**Decision rule: approved with the owner amendment above.** All correctness requirements must
hold. Keep the simpler implementation on an inconclusive result.

1. For each nonempty bulk workload at N = 1,000 and 10,000 with C at least
   ceil(N/10), B1 total-action p50 must be at most 50% of B0 in both timing
   sets. The gain must exceed both variants' IQRs. Total-action p95 must not
   regress beyond the larger of 10% or 2 µs in any cell.
2. At those large bulk cells, estimated action allocations must be at most
   35% of B0. Report lifetime allocations as well; they must not regress by
   more than 10%. Every small bulk cell must stay within the larger of 25%
   or 2 µs of B0 p50, and within 256 additional sampled bytes/action.
3. Existing non-bulk controls, setup and teardown must stay within the larger
   of 5% or 0.5 µs of B0 p50; no meaningful p95 regression beyond the larger
   of 10% or 2 µs. Live retained overhead must be at most the larger of 10%
   or 16 KiB over B0 per collection; post-teardown growth must be at most
   16 KiB over its empty baseline and show no growth over 100 churn cycles.
4. Counted diagnostic runs must show one new exposed keys array per nonempty
   bulk call and no extra subscriber evaluations or deliveries. Source review
   must establish one original-order traversal and O(C) bucket traversal, plus
   supplied record-field copying; timing slopes alone do not prove complexity.
   Rollback must use the saved keys identity and O(C) undo entries. Report
   module bytes and added source lines as costs, without a fabricated size win.

These are per-cell gates, not averages. A mixed workload that fails because
of unchanged item-update/reorder work still fails the proposed rule: report it
and seek an explicit decision rather than excluding that work retrospectively.

**Host, tools and budget.** Node 24.12+ for pure timing and semantics; Chromium
from this package's pinned Playwright for confirmation, allocations and heap.
Before runs record actual versions, OS/architecture, CPU/memory, power and
background load; record the public repository revision and hashes of every
uncommitted measured source and runner. Existing headless timing numbers do
not establish the result on the current source or host. Use local modules and
installed tools only, no network, downloads, product data or external calls.
Each run owns and closes its browser contexts, server and child processes on
success, failure and cancellation. Request host access for loopback/browser
process needs. Run sequentially, with no other heavy suite deliberately running.
Proposed budget: 45 minutes per timing set and 60 minutes total for profiling;
stop and report unfinished cells at the limit, without dropping them from the
decision rule. Agreement authorizes these lab-specific comparisons and their
semantic assertions only, not package checks or unrelated heavy runs.

**Confirmation and integration.** Run decisive B0/B1 cells in both Node and
Chromium; an engine-specific win is scoped as such. After the report, obtain
the owner's adoption decision before changing product code. If accepted,
implement the chosen API with behavioral tests and synchronized architecture,
example and guide changes. Repeat the decisive sequences against the shipping
`@canonic2/lattice/collection` export, including correctness, allocation and
retention cells that determined adoption. A prototype win is not shipping
acceptance. Checks remain subject to the workspace's explicit-request policy.

**Follow-up calibration proposal, 2026-10-26 — not approved.** After the early
stop reported below, propose increasing the action cap only for cheap controls
and membership cells with C at most 1, from 10,000 to 100,000 actions per
block. Keep the 20 ms accumulated action-time minimum, the 100,000-item live
preparation cap, all performance/correctness gates and the original wall-time
ceilings. This changes the run procedure, not the product acceptance limits.
The current runner has not been changed or rerun under this proposal. After
agreement, recalibrate both variants and engines before deciding repetition
counts or launching any main set. Stop again if the amended cap cannot meet
the timing boundary. The observed single calibration block is not used to
choose a winner or substitute for the required repeated samples.

**Owner calibration amendment, 2026-10-26, recorded before continuing.** The
owner approved a 100,000-action cap only for cheap controls and single-change
calibration cells. All other cells retain the 10,000-action cap. Keep the
100,000-live-item preparation limit, original time ceilings and every
acceptance gate. Prepare bounded chunks outside the timed regions, with
identical boundaries for control and candidate. Stop accumulating after the
chunk that reaches 20 ms; do not consume the remaining action allowance.
If the amended cap still cannot resolve a cell reliably, report inconclusive.
The owner authorized continuing within this amended plan without another
approval. This supersedes the preceding unapproved proposal; API adoption and
shipping implementation remain separate decisions.

**Runner preparation, 2026-10-26, before the main sets.** The amended small-cell
calibration reached the duration boundary in Node and Chromium. Before fixing
main-set repetition counts, calibrate the remaining matrix cells with the same
approved per-cell caps; use the larger B0/B1 repetition count for both variants
of each paired cell, then freeze those counts. Oracle results are computed
once outside timing for each deterministic cell and checked against each
fresh fixture after timing. Separate collection construction and subscription
setup counters are outside action timing. Chunks account for inserted items
as well as initial items so simultaneously prepared live records stay within
100,000. These runner details do not change fixtures, logical actions, gates
or budgets. Each timing set has a 45-minute wall-clock deadline, including its
preparation; incomplete cells remain explicitly unmeasured. The read-only
power-state probe was blocked by workspace command policy; record power state
as unavailable rather than substituting a different command.

**Budget enforcement clarification, 2026-10-26, during full-matrix calibration.**
The 10,000-item subscribed preparation is synchronous and can hold the driver
inside one cell for a long period. Add an external, low-frequency watchdog
against the original run start (no budget reset), which verifies the owned
process identity and enforces the same 45-minute deadline even while JavaScript
is inside that cell. Preserve completed cells and mark any interrupted cell
unmeasured. This changes no candidate, workload, metric or gate. No benchmark,
profiler or package suite runs alongside the active calibration.

**Runner correction, 2026-10-26, before main measurements.** The extension to
fresh-fixture calibration of every large subscribed cell was a runner error:
it repeatedly measured the cost of preparing and releasing all subscriptions
before obtaining a few microseconds of write timing. Preserve that partial run
as `runner-corrected`, not a candidate failure; do not erase its data. The
approved small-cell calibration in both engines already resolved with the
amended caps.

For steady-state action timing, prepare one warmed fixture per variant and
restore its logical input state outside each timed action using public
operations. Removed original incarnations receive fresh subscriptions during
restoration; all prior subscriptions end normally. Compare the same actions,
values, survivor identities and delivery counts, and keep input conversion,
validation and commit delivery inside the action timer. Validate fresh
fixtures separately. Prepare and restore bounded data outside timing, apply
the same boundaries to both variants, and retain the 100,000-live-item bound.
Calibrate with the same 20 ms target and amended per-cell caps, then freeze
paired counts. This avoids multiplying full subscription construction by the
steady-state repetition count.

Measure fresh setup, teardown and complete create/action/abort lifetimes
separately, at their own repetition count rather than multiplying those cold
phases by the steady-state count. Keep the required five warmups and fifteen
measured samples. Retention/cleanup probes and the fresh-fixture oracle cases
remain required; this is no relaxation of those gates. Use seed 1 to shuffle
the full matrix before the first main timing so a time-limited partial report
does not always omit the largest sizes. The corrected initial set keeps the
original run deadline, with no budget reset; stop unfinished cells at that
deadline. Prototype code and every acceptance threshold remain unchanged.

### Stage 6: proposed decisive comparison amendment (2026-10-26 UTC)

**Status: Approved by the owner on 2026-10-26 UTC; run reported below.**
The owner approved all 20 cases, independent large-case repetition counts and
reduced allocation/retention sampling. Setup, resets, correctness checks and
counters stay outside action timers; setup is measured separately. Starting
states and logical work must match, with results normalized per action. The
two-minute diagnostics are investigation limits, not grounds to dismiss a
regression. Five allocation profiles are initial evidence; unresolved variation
means inconclusive. Use the initial comparison to decide whether confirmation
is warranted: stop on a clear adoption-gate regression rather than confirming
a rejected candidate. Preserve all earlier evidence and numerical gates;
deferred cells remain unmeasured. No further pause is needed within this plan.

This is a reduced matrix and a bounded measurement procedure. It
does not change the prototype, adopt an API, relax any numerical acceptance
limit, replace the preserved partial reports or begin another roadmap stage.

#### Cases fixed before running

A case is one workload/size/subscription condition, comparing B0 with B1.
Run these **20 paired cases** in Node and installed Chromium. S means the same
representative subscriptions as before: one label/done item watcher per
original item, one membership watcher and one membership-count watcher.
U means no subscriptions. C is the number of keys affected, as in the earlier
generator; replacement has C removals and C insertions.

| IDs     | N             | C      | Workload                                                             | Mode | Paired cases | Why retained                                                                         |
| ------- | ------------- | ------ | -------------------------------------------------------------------- | ---- | ------------ | ------------------------------------------------------------------------------------ |
| B01–B02 | 1,000; 10,000 | N      | Append                                                               | S    | 2            | Growth to twice the original size; bulk construction and order copying               |
| B03–B04 | 1,000; 10,000 | N      | Seeded scattered removal                                             | S    | 2            | Shrink to empty, removal cleanup and scaling                                         |
| B05–B06 | 1,000; 10,000 | N      | Same-key replacement in original order                               | S    | 2            | Incarnation replacement even when final key strings/order match                      |
| B07     | 1,000         | 1,000  | Seeded scattered removal                                             | U    | 1            | Preserve the old unfavorable control-IQR/noise case                                  |
| B08     | 10,000        | 1,000  | Seeded scattered removal                                             | S    | 1            | Pure membership with surviving records/order at the 10% bulk boundary                |
| B09     | 10,000        | 1,000  | Mixed update/remove/insert/reverse                                   | S    | 1            | Genuine mixed work: 1,000 survivors updated, 1,000 removals/insertions, full reorder |
| B10     | 10,000        | 10,000 | Full mixed replacement/reverse                                       | S    | 1            | Preserve the expensive old sentinel; explicitly has zero survivor updates            |
| Q01     | 1,000         | 10     | Scattered removal                                                    | U    | 1            | Exact prior setup-regression condition; full case remains gated                      |
| Q02     | 100           | 1      | Mixed update/remove/insert/reverse                                   | S    | 1            | Exact prior p95 concern; full case remains gated                                     |
| Q03     | 100           | 1      | Scattered removal                                                    | S    | 1            | Preserve the interrupted cell as a named pending comparison                          |
| C01     | 10            | 0      | Empty bulk change                                                    | S    | 1            | One canonical no-op, merging the two old equivalent empty aliases                    |
| C02     | 10            | 1      | Bulk append                                                          | S    | 1            | Cost of the new API for one change                                                   |
| C03     | 10            | 2      | Bulk prepend                                                         | S    | 1            | Preserve the old small two-insertion comparison and stable anchor ordering           |
| C04–C07 | 10            | —      | Existing empty batch, single insert, single remove, item-only update | S    | 4            | Protect ordinary paths, including a true existing-batch control in both variants     |

Use the existing seed-1 generator, records, insertion anchors and expected
results. C04 calls ordinary batch in both variants; it is distinct from C01's
comparison with the proposed bulk API. Initial subscription delivery is outside
action timing and counted separately in correctness runs.

This reduces 184 paired cases to 20, without deleting any old result. It
retains the exact setup and p95 concern cases, the noisy full-removal case and
the interrupted case. Pure prepend/scattered-insert combinations, the other
no-subscription bulk combinations and the 100-item append/prepend/ten-key
replacement/scattered-insert performance cells are **deferred, not passing**.
Their functional anchor/order cases remain in untimed correctness probes.
The two old empty aliases are merged because they supply the same empty API
change set, not because either result is unfavorable. Do not extrapolate a
successful reduced matrix into acceptance of every deferred size/change/mode.

B07 retains the noisy workload but its proposed fresh-fixture condition is a
scope change, not a retrospective cure for the old warmed-run IQR result.
Report those conditions separately. Q02 retains the warmed condition and adds
an instrumented diagnostic; Q01 retains the original fresh setup boundary.

#### Runner boundaries and readiness

Keep the recorded B0 control and B1 candidate hashes fixed. B0 uses the
shipping collection entry point read-only; B1 remains a local prototype.
Readiness smoke must exercise both variants for every selected group, timing,
allocation and retention boundaries, cancellation, failure and cleanup before
a long set. Runner defects stop that set and receive a dated correction;
candidate correctness failure rejects the candidate rather than being tuned
away.

**Clean decision timing.** Disable subscription counters, metadata histories,
profilers and GC observers in every acceptance timing. Keep deterministic
counted runs separate. Use identical declared fields, readers, equality,
callback roles and error policy in B0/B1. The clock starts before API input
conversion and ends after the synchronous commit has delivered. No input
adaptation or validation is shifted out of that boundary.
This applies to subscriptions installed by restoration/rebinding as well as
initial setup; merely changing the initial counted flag is insufficient.
Run the full semantic stress probes in a separate owned validation process or
realm, serialized before timing blocks. They remain required, but must not
feed different object shapes or extra allocation history into the two timing
processes. Keep canonical per-action value/identity/error validation outside
each timer in the timing process itself. This isolation is an explicit runner
amendment; its cost remains in the phase wall budget.

**Large cases B01–B10.** Prepare fresh bounded fixtures outside the action
timer and run one logical action per fixture. Module code is warmed, but the
collection/registration state is fresh. Drop the fixture after post-timer
validation and cleanup; do not restore thousands of keys using repeated
scalar operations. This also restores canonical subscription registration
order on every action. These are fresh-collection action measurements; do not
pool them with the earlier warmed/restored action samples.

Calibrate each large variant independently to 20 ms, with at most 10,000
actions, stopping at the first completed bounded chunk that reaches the
target. Freeze each variant's count before its decision blocks and reuse it
in confirmation. Different repetition counts are an **explicit amendment**:
every invocation has the same measurement boundary and logical workload,
and every variant still has five warmups and fifteen measured blocks.
A slow four-second control is not repeated dozens of times solely because
the candidate needs several calls to accumulate 20 ms. Normalize per call;
report counts and unnormalized block durations too.

**Concern/small cases Q01–Q03 and C01–C07.** Use prepared, restored fixtures
for steady-state action comparisons, with reset outside timing. Keep matched
B0/B1 repetition counts for these small cases. The 100,000-action cap applies
only to cheap controls and C <= 1 calibration; all other cells keep 10,000.
The 20 ms target and 100,000-live-item preparation limit stay unchanged.
Use indexed structural checks and lazy failure diagnostics outside the timer
instead of constructing full reference maps and eager JSON diagnostic strings
on every action. Every action still checks exact values/order, survivor and
keys identities, and errors. Counted preflights additionally check callback
history, ordering, incarnations, abort and at-most-once delivery.

**Cold phases.** Retain the original complete input-generation/create/
subscribe/action/abort boundary and separate construction, subscription and
teardown metrics. Use matched cold counts: 100 lifetimes/block at N <= 100,
five at N = 1,000, one at N = 10,000. For large fresh cases reuse the first
matched cold-count invocations of each action block; add only the required
missing cold invocations when a variant's action count is smaller. Label
those additional calls cold-only, not extra action samples. This avoids
performing a second expensive full action just to measure its lifetime.

After every block persist raw samples and phase wall times; never wait for a
whole cell to finish before saving. Separately account for fixture generation,
construction, subscription setup, action, validation, reset, teardown,
reporting and unclassified time. Verify conservation of the wall-time ledger.
An independent owned watchdog enforces the phase deadline during synchronous
work. A timeout is incomplete evidence, never a latency sample.

#### Bounded investigation of the concerns

Run these first, within a combined **four-minute ceiling**, charged to the
initial timing-set budget:

- **Setup, at most two minutes.** Repeat exact Q01 end-to-end setup with the
  old boundary and matched counts in both variants. In separate diagnostic
  groups measure fixture-input/controller generation alone and construction
  from prebuilt entries. Use three isolated preconditioning regimes: identical
  canonical-record warmup; identical benign empty/getter payload-shape warmup
  in both variants; and reproduction of the old variant-specific semantic
  prefix. Each has five warmup and fifteen measured blocks of 100 constructions
  in seeded paired order, split into bounded chunks. B1's extra old probes
  exercise getter/empty-record inputs through helpers also used by construction;
  unequal JIT/heap conditioning is a hypothesis to distinguish from an actual
  constructor regression, not an explanation established by source alone.
  Report pure-construction and common-harness costs without subtracting
  them from the primary setup result. This distinguishes constructor cost
  from generator/harness/JIT/host variation. Keep the old 378.547/423.309 µs
  setup result visible; it was collected without counted subscriptions.
- **p95, at most two minutes.** Repeat exact Q02 with counters off as the
  decision condition. Compare counters on/off and subscribed/unsubscribed
  versions as separate diagnostic conditions, with seeded paired order,
  five warmups and fifteen measured blocks. These extra diagnostic modes
  cannot become substitutes for Q02. Collect individual operation durations
  into preallocated buffers outside each action's timestamp boundary, while
  preserving block-mean p95 as the original acceptance statistic. Record
  timer resolution and host load. A short separate GC-attribution pass may
  run for at most ten seconds per variant; no tracer or GC observer runs
  during acceptance timings.

The p95 work diagnoses instrumentation, reset/heap history and runtime
variation; none is a presumed cause. Do not exclude outliers or GC-affected
decision blocks, pool away a bad replicate, substitute medians for p95, or
waive a reproduced regression. If the diagnostic ceiling prevents resolution,
report the concern as unresolved and preserve its failed/partial evidence.

#### Allocation, retention and confirmation

After the investigations and initial decision set, collect separate Chromium
memory evidence for the same 20 cases, then repeat all 20 timing cases in an
independent confirmation set in both engines. Keep Node/Chromium conclusions
separate. No product code changes or candidate tuning between these phases.

The old allocation-count heuristic is not affordable at the largest sizes:
a control costing about 4.6 seconds would require over seven minutes for one
100-action profile, before five replicates or lifecycle profiles. Therefore
the following sampling reductions are **proposed amendments, not accepted
results or changed performance limits**:

- Keep five independent profiles per case/variant and 512-byte allocation
  sampling with both minor- and major-collected objects included. For B01–B10
  take one complete action per profile, on independently initialized fixtures;
  one action already creates many order-array allocation samples in the
  control. Report sample counts and uncertainty; if insufficient, mark
  inconclusive rather than reporting zero allocation or increasing work
  unplanned. For Q/C cases keep 1,000 actions/profile in bounded prepared
  chunks. Warm Q/C fixtures and reset before sampling, never inside the
  action-only sampling window. Use identical empty-loop controls to expose
  profiler/harness attribution.
- Profile complete create/action/abort lifetimes separately, with the same
  large/small action counts. Keep fixture/oracle diagnostics outside sampling;
  preserve input generation inside the complete-lifetime boundary. Do not
  confuse sampled allocated bytes with retained heap or exact byte totals.
- Keep five independent retained-heap samples per case/variant for commit and
  rollback: empty, initialized, completed with live collection, and
  aborted/dropped collection. Drop runner inputs, oracle references and
  histories before post-action heap measurements. Measure caller-held old
  keys separately at B06 and C02, in five samples each, rather than retaining
  old arrays accidentally in every sample.
- Keep 100-cycle churn at B06 (10,000-item same-key replacement, subscribed)
  and Q02 (partial mixed state, subscribed). These exercise maximal
  incarnation/signal cleanup and touched-record/batch bookkeeping respectively.
  The other selected cases get the five commit/rollback retention samples,
  but are not individually claimed to have passed 100-cycle churn. Churn
  coverage is explicitly reduced, not its cycle count or retained-growth gate.
- Record up to five forced-GC observations at each retained snapshot. Keep all
  observations. If their range can change the memory-gate outcome or weak
  references remain live for reasons the probe cannot resolve, report
  inconclusive. Do not choose a favorable minimum heap value.

Correctness, 2× membership-only speedup, 65% allocation reduction, mixed-action
non-regression, small-operation limits, setup/teardown limits, live/post-drop
memory limits and meaningful-difference rules remain exactly as agreed.
Diagnostics never override a failing decision case. Confirmation uses the
same cases, scopes, source, boundaries and frozen repetition counts and
reports each set separately. All required selected gates must hold before an
adoption recommendation. Deferred cells do not become passed gates.
Any eventual adoption remains a separate owner decision, followed by behavioral
tests and decisive shipping-path comparisons; this plan does not authorize it.

#### Cost estimate and ceilings

These are planning estimates from preserved costs, not new measurements:

- The old 10,000-item full mixed cell has a fresh control action median of
  4.584 s and a complete control lifetime median of 5.239 s; B1's complete
  lifetime median is 0.948 s. Twenty matched fresh rounds therefore cost
  about 124 s per engine for this sentinel, before preflight/calibration,
  instead of paying quadratic reset plus a second lifetime pass. The old
  control lifetime p95 was about 30 s, so this is not a guaranteed bound.
- Full 1,000-item removal's fresh control action median is 37.81 ms.
  Scaling its visited-key count predicts roughly 3.8 s for full 10,000-item
  removal. The preserved 100-item append cost scales to roughly 0.9 s at
  10,000 full appends; full replacement is estimated at 4–6 s from removal
  work and the old full-replacement sentinel. Those are inferred estimates
  for unmeasured cases, not performance claims.
- Six subscribed bulk endpoints, the two partial cases, noisy-removal
  recheck and full sentinel should cost roughly 8–12 minutes per engine
  with independent large-case counts and no quadratic reset. Small cases,
  diagnostics, preflight, validation and reporting add roughly 5–10 minutes
  across the initial set. Budget for Chromium using the Node cost envelope,
  not an assumed browser speedup.
- Memory sampling at one large action/profile gives approximately 4–6 minutes
  of unprofiled action/lifetime work; allow 12–22 minutes for allocation
  sampling and its controls. Five commit/rollback samples and the two
  caller-retention sentinels add roughly 5–10 minutes. B06's 100-cycle churn
  costs about ten minutes for the variant pair under the old lifetime
  envelope; allow 12–18 minutes including GC/driver overhead. The estimates
  are deliberately approximate because profiler and GC costs are unmeasured.

| Phase                                                                        | Estimated duration | Unchanged hard ceiling        |
| ---------------------------------------------------------------------------- | ------------------ | ----------------------------- |
| Diagnostics, smoke, calibration and initial 20-case timing set, both engines | 25–35 minutes      | 45 minutes total for this set |
| Allocation and retention, including selected churn                           | 30–50 minutes      | 60 minutes total              |
| Independent confirmation, same 20 cases in both engines                      | 20–30 minutes      | 45 minutes total for this set |

Total estimate: **75–115 minutes**, versus the unchanged 150-minute aggregate
maximum of the three ceilings. Phase budgets are not transferable and do not
reset on a runner correction. Start each watchdog before smoke/preparation.
Record phase costs continuously so an estimate cannot silently become a new
allowance. Stop on correctness failure, unresolved calibration, prerequisite
failure or the phase ceiling; preserve partial work and label unfinished cases.
No additional matrix, candidate, sampling interval or threshold change is
implicitly authorized by approving this proposal. No network, downloads,
external data, other product edits or package checks are included.

### Stage 6: proposed setup-tail and bulk diagnostic amendment (2026-10-26 UTC)

**Status: Approved by the owner on 2026-10-26 UTC; run reported below.** The
owner clarified that each fresh process runs both variants, interleaving
their blocks and alternating which runs first. Keep process results separate;
p95 denotes p95 of block-average setup times, not individual setup latency.
Proceed through both phases without pausing on a setup performance result.
Keep the
prototype unadopted, the measured control/candidate sources unchanged, all
original reports intact and every numerical adoption gate unchanged. This
amendment asks two questions: does Q03's setup tail repeat across fresh
processes, and do the intended large membership operations show the expected
action-time benefit? Bulk diagnostic results cannot override a failed setup
gate or establish adoption without the absent memory/confirmation evidence.

#### Fixed scope and process isolation

Use Node 24.12.0 on the same host, offline, without package checks, downloads
or product edits. This is a Node-only diagnostic amendment, not completion
of the earlier Node/Chromium matrix. Record source hashes, runtime, clock
resolution and host/load conditions before measurement. Preserve all raw
blocks in a new owned results directory. Save after every block and enforce
deadlines from a coordinator able to terminate its own synchronous children.

Before measurements, smoke the three bulk operation groups and the exact
Q03 setup/subscription lifecycle at small sizes in a separate validation
process. Keep semantic stress, counted delivery probes and correctness
checks outside timing processes or after action timers, as appropriate.
Check failure/cancellation cleanup before the bounded sets. Correctness,
ownership, source mismatch or runner failure stops the run. An observed
setup performance failure does **not** skip the three bulk diagnostics.

#### Setup tail: one case, three independent fresh processes

Use Q03 exactly: 100 initial items, one seeded scattered removal, one item
watcher per original item plus membership and size watchers. Measure fresh
setup separately from the removal. Its primary boundary remains input
generation, collection creation, owner/controller creation and subscription
registration; initial delivery is included. Abort, removal, resets, counters,
correctness checks and IPC stay outside the setup timestamp boundary.
Measure the complete teardown separately with the original abort boundary.
Keep the existing construction/subscription breakdown without subtracting
components from total setup. Identical starting values, order and declared
watcher roles are required for both variants.

Create **three sequential independent fresh OS processes**, each importing
both measured implementations and running paired B0/B1 blocks. Do not use
worker threads as a substitute for process isolation. Only one child executes
measurement work at a time. Strictly alternate which variant runs first on
successive paired blocks, with equal first positions over measured blocks;
alternate the initial first variant across processes. Close each process
before starting the next. No asymmetric
semantic preconditioning, forced GC, observers or profilers in this set.

Each process gets **20 warmup blocks and 200 measured blocks of 100 fresh
setups/teardowns**. Thus each variant has 200 block means and 20,000 setups
per independent process, with 60,000 measured setups across three processes.
This is fixed work: no calibration or repetition growth. At 200 blocks,
nearest-rank p95 is order statistic 190, rather than the maximum of fifteen.
Report every process separately: p50/p95/max/IQR for setup, construction,
subscription and teardown; retain paired differences and chronological
blocks. Do not pool processes into a single favorable percentile. Report
tail uncertainty using a seeded moving-block bootstrap (ten consecutive
blocks per resampling unit, 2,000 replicates), qualifying its limitations
for longer correlated pauses. Uncertainty does not change the acceptance
threshold or authorize extra samples.

References to a process pair below mean its paired B0/B1 results within one
fresh process, following the owner's clarification.

Use the unchanged setup/teardown allowances: p50 at most max(5%, 0.5 µs)
and p95 at most max(10%, 2 µs) worse than control. Report the numerical gate
outcome for each process pair. Call the setup-tail difference **repeated**
if the p95 allowance is exceeded in at least two of three pairs; report all
three, including discordant results and uncertainty. Call it not reproduced
under this process protocol only if all three fit the allowance; otherwise
call it inconsistent/unresolved. These are diagnostic repeatability labels,
not replacements for adoption gates or a retrospective pass of the failed run.

Reserve at most **30 seconds within the setup budget** for one additional
instrumented fresh-process pair, at most fifteen seconds per variant, on
this same case only. Capture GC events and CPU samples to relate pauses to
the existing construction/subscription/abort phases. These traces are
diagnostic and never enter primary percentiles. Attribute a regression only
when repeated primary excess, phase breakdown and trace evidence support
the same mechanism; temporal GC correlation or a sampled hot function alone
is not proof. If evidence is absent or conflicting, name the unresolved
cause rather than expanding into new component or workload matrices.

#### Bulk diagnostics: exactly three intended operations

Run these regardless of whether the setup allowance is exceeded, using
separate fresh OS processes from the setup investigation. Fix the order
below before running, with interleaved control/candidate blocks within each
case, both variants within each fresh case process and no concurrent
measurement work. Strictly alternate which variant runs first:

| Case                     | N      | C      | Subscriptions                                     | Purpose                                                      |
| ------------------------ | ------ | ------ | ------------------------------------------------- | ------------------------------------------------------------ |
| B02 append               | 10,000 | 10,000 | Same representative item/membership/size watchers | Grow to twice the initial size; avoid repeated order copying |
| B04 scattered removal    | 10,000 | 10,000 | Same watchers                                     | Remove all items; include commit cleanup                     |
| B06 same-key replacement | 10,000 | 10,000 | Same watchers                                     | Preserve final key order while replacing every incarnation   |

Use the existing seed-1 generator, input conversion and sequential oracle.
Each case gets **two warmup and seven measured paired blocks, one logical
action per variant per block**. Prepare a fresh fixture outside each action
timer; preserve equivalent values, order, subscription registration and
logical work in B0/B1. Start action timing before API-specific input conversion
and end after synchronous commit delivery. No reset, reference capture,
counters, setup or validation inside action timing. Validate exact values,
order, frozen/stable old keys, surviving identities, replacement incarnations
and errors after each action; counted preflights separately verify deliveries.
Abort every owned fixture and save setup/action/validation/teardown wall costs.
Keep at most one prepared fixture per variant; the 100,000-live-item ceiling
is unchanged. Do not use the prior fresh helper's capture-contaminated lifetime
span as a clean lifetime measurement; this set reports setup and action
separately and makes no complete-lifetime claim.

There is **no calibration**. One invocation is one action, normalized per
action identically for both variants. Report raw action durations, p50,
p95/max (seven samples make p95 the maximum), IQR, speedup and the previous
membership-only timing target: candidate p50 at most half control, with the
median gain exceeding both IQRs. Results are expressly **diagnostic**, with
no confirmation or allocation/retention acceptance claim. If a single call
is too short to resolve a difference, label it inconclusive; do not accumulate
more calls. Do not add mixed, prepend, small controls, other sizes, modes or
engines. Their existing evidence and deferred status remain unchanged.

#### Budget, stop conditions and report

| Phase                                                     | Estimate from preserved costs | Hard wall ceiling |
| --------------------------------------------------------- | ----------------------------- | ----------------- |
| Readiness, setup-tail pairs and optional attribution      | 30–90 seconds                 | 3 minutes total   |
| Three bulk diagnostics, including preparation and cleanup | 2–4 minutes                   | 5 minutes total   |

The estimate uses about 88 µs setup and 44 µs teardown per 100-item lifetime:
132,000 measured-plus-warmup lifetimes across three processes imply roughly
17 seconds before process/IPC/validation and tracing overhead. Bulk estimates
remain inferred: roughly 0.9 s for full append, 3.8 s for removal and 4–6 s
for replacement in the control. Nine rounds give roughly 78–96 seconds of
control action work, before candidate work, setup, smoke and cleanup. The old
30-second cold-lifetime tail makes those estimates uncertain; the watchdog
limits spending rather than extending the matrix to compensate.

Maximum is **eight minutes**, with no transfer or reset between phase ceilings.
The setup deadline stops that investigation, preserves incomplete evidence
and proceeds to the precommitted bulk diagnostics if correctness and ownership
remain sound. A bulk deadline preserves unfinished cases as unmeasured.
No source tuning, matrix expansion, repeated failed run, memory phase or
confirmation is included. Report setup repeatability per process, each bulk
operation's diagnostic benefit/uncertainty, any attributable regression and
the limits of that attribution. Keep adoption pending regardless of outcome.

### Stage 6: implementation checkpoint and proposed continuation (2026-10-26 UTC)

**Status: implementation checkpoint prepared; continuation authorized by the
owner's 2026-10-26 checkpoint instruction.**
The owner requested a reviewable implementation and correctness tests, preserving
normal shipping behavior and keeping acceptance pending. The actual opt-in
entry is `@canonic2/lattice/experimental/collection`; the regular export still
targets the unchanged accepted source. No root export or supported API guide
was extended. The unpublished package explicitly labels the opt-in experimental.

`src/modules/collection/experimental.js` initially ports the prototype's runtime
logic, with typed experimental membership changes, bounds annotations and a
relocated reporter import. Its initial hash is
`c45e032bca979be04c6f285ea0a44a24e81883af902614693a6eeaa45ede0f7d`;
the package manifest hash is
`fb72313f5a16fa52cac9ca79f8e015b26aa96ef221a9392a3772d4492b07e868`.
The core is separate so the accepted module, import graph, instance shape and
ordinary-call costs do not change during evaluation. Consolidation is an
acceptance/migration decision, not an implicit change in this checkpoint.

Correctness tests are authored beside the checkpoint, using its actual package
entry. Packed-consumer and Node import fixtures verify the regular collection
still lacks the experimental operation. Type fixtures cover item inference,
valid operations and invalid item/key/anchor/option calls in Node/browser type
environments. Their broader execution remains unverified. The lab's separate
correctness probes exercise the actual entry; authored test coverage is not a
completed package acceptance result.

#### Preservation and comparison inputs

The measured baseline, reporter and prototype were copied byte-for-byte into
a separate local frozen source tree with their original relative module
layout and a manifest, refusing to overwrite existing files. Their SHA-256
hashes remain the previously recorded baseline
`b2afe023ca3a1ebd81b741e6726273c97ce03a860702496fa70c8db928606f76`,
reporter `e4ad2ac9d60e185067e23107b1133b5f5067dbc16c5b4b9dc41401ce3ad2104d`
and prototype `22c3d04f416610f613ad383cd7f7e9738458adbf3ab339d19d69cf293aaeaacf`.
Original reports and their hashes remain untouched. The initial checkpoint
source/manifest are preserved separately before any corrective edit. Later
checkpoint revisions must get distinct hashes and saved reports; never replace
the original comparison source with a tuned checkpoint.

A local lab adapter now composes B0 from the frozen measured baseline and B1
from the actual experimental package export. It verifies all frozen source
hashes before entering the existing fixture/measurement protocol. Old runners,
prototype and reports are not rewritten. Browser comparisons must explicitly
resolve the manifest's experimental export to the same source bytes, recording
that mapping, rather than continue importing the original prototype.

#### Proposed bounded teardown attribution

This changes the attribution target from the old setup case to the observed
large-append abort regression, and changes B1 to the checkpoint package entry.
The owner's instruction explicitly requests this investigation against the
checkpoint and subsequent remaining measurements. The reproduction uses the
already selected workload, counts, fresh-process protocol and eight-minute
ceiling; separately instrumented diagnosis cannot change acceptance gates.
It introduces no new numerical acceptance threshold or claim of adoption.

Use Node 24.12.0 on the same host. Fixed case: 10,000 initial items, append
10,000, representative item/membership/size watchers; all original watchers
remain alive until abort. Compare B0 frozen baseline and B1 checkpoint in
three fresh OS processes, both variants interleaved within each, strictly
alternating which runs first. Two warmups and seven measured paired blocks
per process, one action per fixture, no calibration. Preserve equivalent
starting values, order, watcher registration and action. Prepare references,
run correctness checks and count deliveries outside action/abort timers.
Measure setup, append action, synchronous abort and validation separately.
Use no profiler, GC observer or forced GC in the primary set. Save each block,
report each process separately and retain every unfavorable sample.

Then take one separately instrumented paired append/abort sample through the
same entry points, starting CPU and GC observation only for attribution,
never using its timings as acceptance evidence. Take three additional paired
diagnostic samples with explicit pre-abort GC, alternating ordinary abort and
pre-GC abort within a fresh process to distinguish allocation/heap scheduling
from repeated release/unlink work. GC is outside abort timing; report its
duration separately. Do not subtract pauses from primary times or treat a
single profile/hot function as causal proof. No other workload, count, mode,
engine or forced-GC variation is included in this attribution amendment.

Keep all teardown limits: p50 at most max(5%, 0.5 µs), p95 at most
max(10%, 2 µs) above B0. Seven-sample p95 equals maximum and is qualified as
such. Repeatability requires the failed numerical gate in at least two of
three primary processes; inconsistent results remain unresolved, not passing.
Any identifiable mechanism must be supported by the repeated primary excess,
phase/profile evidence and the controlled diagnostic before changing code.

Estimate 3–6 minutes from the saved 4.572 s baseline append, roughly
0.7 s setup and 0.24/0.63 s abort costs. Hard ceiling **eight minutes**,
including readiness, source verification, preparation, attribution and cleanup.
One owned coordinator watchdog stops synchronous child work at the ceiling.
No calibration growth, additional cells or automatic rerun after a runner
correction. Timeout means incomplete evidence. Correctness/ownership/source
failure stops immediately. A performance failure remains evidence to explain,
not a reason to hide the intended comparison.

#### Corrections and remaining acceptance phases

**Checkpoint correction recorded before admission measurements.** The initial
checkpoint reproduced the large-append abort regression in all three fresh
processes (candidate medians 627.171/679.649/621.485 ms versus baseline
278.164/268.516/266.461 ms). CPU profiles place 564 of 577 candidate samples
in native `removeEventListener`, with zero observed GC during abort. Pre-abort
GC did not remove the excess. Once-listener dispatch has already removed the
current listener, but the release path redundantly searches the remaining
signal list for it. This is avoidable work shared with the old implementation;
the evidence identifies the dominant cleanup mechanism, not every reason its
cost differs between heap histories.

The experimental abort callback now clears its signal/onAbort registration
references before release. Other release paths still detach live owning
listeners. The ordinary module and frozen baseline/prototype remain unchanged.
Corrected checkpoint hash:
`7610e112f6a165f95bbcd13903c92d50e30853729499aca5a884926388405af1`.
The initial/corrected snapshots and separate reports were retained. The same
three-process reproduction then measured candidate abort medians
3.434/4.261/3.646 ms versus baseline 293.135/263.105/276.542 ms; every abort
p50/p95 gate fit. The candidate profile no longer has native listener removal
as its dominant sampled cost. Both reproduction sets completed within their
eight-minute per-revision ceilings; no runner correction reset a deadline.
Admission timing uses this corrected hash, not the original prototype.

Make only attributable corrections in the experimental module; never edit the
accepted module or frozen sources to improve the candidate result. Preserve
the initial checkpoint and every corrected revision. Add behavioral tests for
the mechanism and repeat the decisive teardown comparison through the actual
package entry with the same source/boundaries/counts per recorded revision.
An unresolved attribution is reported explicitly rather than prompting a
speculative optimization or erasing a gate.

The previously approved twenty-case matrix, independent large-case counts,
small/concern controls and numerical gates remain the acceptance scope. The
checkpoint has not completed that matrix, so do not relabel the three bulk
diagnostics as a complete initial comparison. After a justified correction,
the remaining evidence is: checkpoint initial twenty-case timings in Node and
Chromium (45-minute ceiling); the same twenty-case allocation/retention plan
(60-minute ceiling); independent twenty-case confirmation (45-minute ceiling).
Keep the approved five allocation profiles initially, one action/profile on
large cases, 512-byte sampling including collected objects, five retained
samples per case/variant for commit/rollback, caller-held keys at the named
sentinels and 100-cycle churn at B06/Q02. Unresolved variation is inconclusive,
with no automatic sampling increase. No deferred cell becomes passing.

Use the initial comparison to decide whether memory/confirmation is warranted;
do not spend those budgets on a clearly rejected candidate. Phase ceilings
are not transferable or reset by runner corrections. Any correction receives
its explicit comparison/revision record before rerunning; it does not silently
restart an exhausted phase. Source changes or further workload/sampling
amendments require agreement before measurements. The authoring request does
not adopt the checkpoint or publish bulk membership as supported.

### Stage 6: independent memory evidence and setup profiling (2026-10-26 UTC)

**Approved by the owner's 2026-10-26 follow-up instruction.** Allocation and
retention now proceed independently of the failed setup gate. Adoption remains
blocked, all original gates/evidence remain intact, and confirmation is not
authorized to override the failed gate. This explicitly supersedes the earlier
admission prerequisite for the memory phase only.

Use the unchanged twenty selected cases, frozen measured B0 and corrected
experimental package-path B1 (hash `7610e112f6a165f95bbcd13903c92d50e30853729499aca5a884926388405af1`).
The separately authorized cleanup correction in the regular collection/store
does not replace the frozen baseline. Preserve both current and frozen source
hashes in the report. No candidate tuning or matrix expansion during memory
measurement. Run profiled phases sequentially to avoid CPU interference.

Memory keeps its previously approved **60-minute combined ceiling**, five
independent allocation profiles per case/variant at 512-byte sampling including
minor-/major-collected objects, action-only and complete create/action/abort
lifetimes plus matched controls. Large cases get one action/profile; Q/C get
1,000 in bounded chunks with at most 100,000 simultaneously prepared items.
Five retained samples per variant/case cover commit and rollback with empty,
initialized, live-after-action and dropped snapshots. Keep all five forced-GC
observations at each snapshot; if their range can change a gate decision,
classify it inconclusive. Caller-held keys remain separate at B06/C02; B06/Q02
retain the two 100-cycle churn sentinels. Weak-reference release probes cover
commit and rollback. Insufficient sampling remains inconclusive, without adding
profiles or relaxing limits. An allocation/retention failure is reported but
does not silently suppress the other independent memory evidence; correctness,
ownership or deadline failure still stops the phase and preserves partial work.

Before that memory phase, investigate setup directly with source inspection
and a **three-minute ceiling** for separate setup-only CPU and allocation
profiles. Fixed case is Q02's 100-item subscribed initialization; no mixed action,
reset or abort inside the profiling window. Three fresh Node processes each
profile both variants in alternating first order, using 1,000 prepared setups
per profile and no more than 100,000 live items. Warm identical canonical input
shapes in each variant first. Separate CPU from allocation profiling; keep GC
events for correlation and record profiler overhead as diagnostic, not timing
acceptance. Source diff and sampled call/allocation sites distinguish collection
construction, record conversion, listener registration and initial reads.
The checkpoint's additional closure/property is a known source difference;
do not assume it explains the setup tail. This is not another identical
unprofiled timing run and cannot waive the original p95 failure. If attribution
is unresolved within the ceiling, preserve it and proceed with memory.

Regular collection/store once-listener fixes are a separate correctness change
under the owner's explicit instruction. The collection clears the already-ended
registration during its abort callback; the store avoids redundant removal when
an initial callback already aborted and deactivated its subscription before
throwing. Live-signal failure cleanup still detaches, callbacks remain ended,
and original errors propagate. Their regression coverage belongs with each core.
These independent fixes do not adopt bulk membership.

**Runner correction before completing small action profiles.** The first memory
coordinator prepared fresh Q/C action fixtures instead of warming and resetting
them as approved. Large fresh-case profiles and complete-lifetime profiles
retain their intended boundaries. Preserve every original row; classify the
affected small/concern action profiles as exploratory only, regardless of their
values. Warm, validate and reset Q/C fixtures outside sampling in the corrected
probe, including uncounted replacement subscriptions. The continuation reuses
all completed valid profiles by case/variant/replicate/scope/control key and
collects only the missing valid profiles. It writes a new report linked by
hash to the preserved partial report, with exactly the original sixty-minute
deadline. The old probe/coordinator bytes are archived and hash-verified.
Cancellation closed the owned browser, but the old coordinator awaited a stale
CDP promise; it was stopped after verifying no child processes remained, with
its pre-stop report preserved. The continuation races target operations against
cancellation so deadline/cleanup cannot leave that same stale wait. This is a
runner correction, not a threshold, sampling-count or budget amendment.

### Stage 6: single-append diagnosis and separate retention calibration (2026-10-26 UTC)

The owner requested scoped commits, profiling of the additional 375.871
bytes per single append, an attributable correction, and retention calibration
before another substantial run. The measured experimental bytes are preserved
in public commit `a92011d64fe6c979c9b6450b101d15daec8ac856`; independent cleanup,
regression coverage and findings are separate commits. Frozen B0/prototype and
earlier raw reports remain intact. The regular API is not changed by this work.

This is a bounded diagnosis, not a new adoption comparison. First profile C02
through the actual experimental package export, with the unchanged frozen B0:
five profiles per variant, 1,000 warmed/restored actions per profile, 512-byte
sampling including collected objects. Preserve full allocation stacks/lines.
Preparation, reset, validation and abort remain outside sampling, and each
chunk respects 100,000 live items. Alternate first variant by replicate.
Use a three-minute ceiling per before/after diagnostic, without adding samples
or extending the matrix. Inspect attribution before editing; after an
attributable correction, repeat the same probe. Keep the 256-byte allowance
and report observed ranges. This cannot clear other gates or authorize adoption.

Calibrate retention independently, with a separate three-minute ceiling and
no full matrix. Fixed shapes are C01's subscribed empty action at ten items
and a diagnostic subscribed full replacement at 1,000 items. Compare frozen
B0 and the package candidate in separate fresh realms under canonical warmup
and ten exact create/action/abort warmups (including commit and rollback).
Keep all five forced-GC heap readings before and after each of three measured
lifetimes. Report every residual against the original 16 KiB budget, without
warming until a favorable result appears. In each realm also hold/release a
128 KiB numeric-array positive control: distinguish failure to resolve retained
objects from product/runtime residue. Keep control observations separate from
product observations. A first persistent post-drop failure gets one bounded
heap snapshot for retainer/category inspection; no repeated snapshot search.
This calibration cannot establish retained-heap acceptance or waive previous
failures. Preserve timeouts/partial results and stop rather than expanding it.

The first allocation correction makes duplicate/removal lookups conditional.
Its five-profile reproduction reduces the mean gap but leaves the 256-byte
decision inconclusive over observed ranges. Preserve that intermediate source
and report; do not call it passing. The remaining sampled map/bucket is also
unnecessary with one insertion. A second bounded correction stores that one
anchor directly and uses the already prepared incarnation, retaining bucket
lookup for multiple insertions. Repeat only the same fixed C02 reproduction;
no other cell, count, threshold or budget changes.

The first calibration's nominal 128 KiB positive control used 16,384 small
integers; Chromium's compressed representation made its backing storage about
64 KiB. Preserve that complete run as exploratory calibration, including its
baseline drift. Correct to 32,768 integers for the specified 128 KiB backing
store and repeat the fixed calibration once within the original three-minute
deadline. This is a control-fixture correction, not a relaxed byte gate or an
extension of calibration time. Preserve the original runner bytes separately.

### Stage 6: final accept-or-discard stage (2026-10-26 UTC)

The owner's final instruction authorizes correcting retention measurement,
focused final acceptance and affected correctness/package-consumer tests, then
integrating only an accepted result or removing the experimental runtime.
Inconclusive evidence means reject for this version. No permanent experimental
entry or duplicate implementation remains after this stage. This supersedes
the earlier decision to leave the checkpoint available, not any numerical gate.

Fix the scope before running: C02 small append allocations (five 1,000-action
Chromium profiles with 512-byte collected-object sampling); C02/Q02/Q03 setup
and teardown (100 independent lifetimes/block, five warmup and fifteen measured
paired blocks); B02/B04/B06/B09 large append/removal/replacement/mixed actions
in Node and Chromium (five warmup and fifteen measured blocks). Calibrate each
large variant separately until 20 ms total action time, bounded by its existing
cap, and freeze its counts for independent confirmation. Preparation, reset,
oracles, counters and correctness remain outside action clocks. Cache immutable
timing oracles before measurements to avoid repeated quadratic harness work;
timing fixtures and membership work remain identical to their prior definitions.
Confirmation repeats these setup and large cases in fresh Node processes and
Chromium contexts, with identical counts and boundaries. Keep both sets separate.

Retention correction: install one fixed CDP dispatcher and reuse its function
declaration instead of compiling a new browser evaluation wrapper at each
boundary. Warm exactly ten lifetimes of each measured shape, including both
commit and rollback, plus the measurement dispatcher/control/GC path before
empty baselines. Never warm until a favorable number appears. Drop API inputs,
oracles, old references and callback captures before live/drop observations;
do not cache them across retained samples. Release probe remote-object groups.
Keep five GC readings per state. Pair a no-product probe control with each of
five commit/rollback samples at C02, B06 and Q02. A 128 KiB held/released control
must resolve its backing store and return within 16 KiB. Keep raw product and
probe deltas separate; do not subtract a failing raw gate into a pass. Capture
one matched before/after heap pair if residue remains beyond the budget, to
distinguish runtime/probe footprint from reachable collection state.

Retain B06 and Q02's original 100-cycle Chromium churn, five GC observations
at every cycle and original-empty as well as first-cycle growth. Record every
sample, spike and control drift. Node runs the pure semantic/retention tests;
Chromium owns CDP allocation/retained-heap acceptance, as in the contract.
All original correctness, allocation, setup/teardown, 2× membership speedup,
meaningful-IQR difference, mixed-action non-regression, 16 KiB drop/churn and
10%/16 KiB live-overhead gates remain. Any failed or unresolved required gate
prevents acceptance; deferred earlier cells do not become passing evidence.

This final stage has non-transferable ceilings of eight minutes for initial
timing/smoke/calibration, twelve for memory including correction/control work,
and eight for confirmation. These are below the previous 45/60/45-minute caps;
the memory ceiling also fits the unspent portion of the original memory budget.
Do not reset any deadline on a runner correction, expand the matrix or tune
the candidate mid-run. Continue the fixed independent evidence groups despite
a numerical failure; stop a group on correctness/ownership failure or its
deadline. Partial/insufficient evidence causes rejection, not another amendment
or calibration campaign. Correctness/package tests run outside measurements.

The initial timing ceiling expired after the Node cases and Chromium setup
cases, during Chromium large-append calibration. Preserve the full partial
report and original coordinator bytes. The final independent timing group
starts Chromium first, counterbalancing engine order within its unchanged
eight-minute ceiling. Reuse every available per-engine count. If a count was
not reached in the initial set, calibrate that named cell with the same rule
and label it a first final set, not independent confirmation. This does not
drop a selected case or turn missing paired evidence into acceptance; any
unconfirmed required comparison still means rejection for this version.
An available calibration count is not a completed initial timing set: the
final assessment also labels Chromium append unconfirmed because its initial
samples did not finish, even though its counts were saved. Raw reports stay
unchanged; confirmation classification depends on complete paired samples.

### Stage 6: full-replacement retained-object diagnosis (2026-10-26 UTC)

The owner requested full-replacement heaps for the actual preserved candidate
and equivalent controls before selecting a new explicit bulk-operation design.
This authorizes a narrow ownership diagnostic, not another performance matrix.
Use unchanged candidate bytes `406e39c85a96d6b9b341fc62647c3c33e41324e60d00f8a0f52cc9dbd0d0b116`
from its preserved revision, without restoring its public export. Compare the
current regular collection with the delivered cleanup fixes, hash
`1cbd0b38c8a9274c00c5a8f2692319ce84886a659ff69eef5786510fd5b96907`.
This deliberately makes cleanup equivalent rather than reusing the earlier
pre-cleanup control. Preserve all earlier controls/reports separately.

Fixed scope: one full replacement at N=C=10,000, the existing seed-1 removal
order, original insertion order, records and 10,002 subscriptions. Capture
both commit and rollback, in four separate Chromium realms (two variants each),
after ten exact warmup lifetimes. Six heaps per realm: warmed empty, initialized,
after replacement/rollback with explicit caller snapshots, aborted subscriptions
with only the collection retained, all caller/product references released,
and one further fixed five-GC observation after release. Keep all five used-heap
readings before each snapshot. The held collection stage distinguishes abort
from disposal; the final stages clear both collection and owner references.
Use the same fixed dispatcher, release transient remote groups, return scalars,
and keep heap files/identity sets in the coordinator, not the browser.

Track actual heap node IDs for collections, incarnation-shaped objects, item
records, subscription records and caller-held arrays. Follow strong retaining
edges for surviving IDs, excluding weak edges, and report shared code/native/
shape categories separately. Snapshot observer effects remain visible through
the second fixed post-drop observation; do not repeatedly collect until clean.
Stop after these four realms or a five-minute ceiling, including preparation
and captures. Never reset that deadline or relax the original memory gates.
If identity/root evidence is incomplete, report that exact limitation rather
than attributing byte deltas. No timing/allocation comparison or API adoption
is part of this diagnostic.

### Stage 6: direct integration focused comparison (2026-10-26 UTC)

**Authorized scope.** The owner requests a straightforward direct integration
and bounded comparison, retaining the operation-local module planner. This
records the selected cells and unchanged decision limits before execution.
It does not approve a new retention criterion or supported API.

**Design.** Keep `collection.updateMembership(changes)` as an ordinary detached
method with direct lexical access to the current map/journal. Move validation,
input copying and order construction to a module-level planner receiving
`items`, `keys` and `changes`. The method guards planning getters, then applies
the returned call-owned plan through the existing journal/commit path. Accept
one method closure/property per collection as an explicit measured cost; no
private reorder token, dispatch frame, registry or state-carrier migration.
Ordinary operation bodies remain unchanged. Empty input validates before plan
arrays; a single insertion avoids sets/buckets. A returned plan object is an
operation allocation, charged to the existing small-append allocation limit.

**Question/variants.** Does that direct integration preserve cheap ordinary
construction/operations and the intended large-operation gains? B0 is the
current regular collection including cleanup fixes. B1 is a new ignored lab
copy built from that source with the direct method and extracted planner.
Preserve both hashes and the prior candidate bytes/results; this is not a
remeasurement or relabeling of the discarded candidate. No shipping export
or runtime is modified. One real candidate, no allocation-elimination variant.

**Cells, chosen before measurement.** Constructor-only setup at 10 and 1,000
items isolates the added method/property; subscribed setup at 100 items covers
the prior setup-tail concern. Small actions are C01 empty, C02 single append,
C07 ordinary item update, each at ten items. Large actions are B02 append,
B04 removal, B06 replacement (N=C=10,000), and B09 mixed updates/membership/full
reorder (N=10,000, C=1,000). These isolate the known cost and the three intended
bulk benefits while preserving a total mixed action. Deferred sizes/workloads
remain unmeasured; no matrix expansion follows an unfavorable cell.

**Boundaries/runs.** Node 24.12+ and the existing pinned Chromium, separately.
Seed-1 fixture, oracle, record/reference capture, resets, correctness and counters
stay outside action clocks. Prepare identical starting states independently.
Use one smoke per selected cell plus existing semantic/cleanup probes first.
Constructor inputs are prepared in bounded chunks outside its timer; count only
constructor calls, not fixture input generation. Subscribed setup uses identical
fixture construction boundaries. Use alternating B0/B1 order, three warmup
blocks and fifteen measured blocks. Calibrate each variant independently until
20 ms of measured work, at most 100,000 actions for cheap constructor/control
cells and at most 100,000 prepared live items at once. Small action reset stays
outside its clock. Large actions use one fresh action per block: report their
individual p95; setup/small block p95 is explicitly block-average p95. If a cell
cannot reach 20 ms within the preparation/cap/time bounds, retain it as
inconclusive, never a zero-cost win. Calibration has a fixed 30-second ceiling
per cell/variant, charged to the overall ceiling; do not expand it.

**Allocation.** Chromium C02 only: five alternating independent HeapProfiler
profiles per variant, sampling interval 512 bytes with collected objects
included, 1,000 actions/profile. Prepare/reset/validate outside profile start/
stop; aggregate sampled action bytes per action. Profile calls and returned
trees belong to the coordinator. Store raw profiles; treat inadequate samples
or overlapping variation as inconclusive. Other allocation/lifetime/churn and
new full-replacement snapshots are not part of this focused comparison.

**Metrics/rules.** Per-action p50, nearest-rank p95, max/IQR and setup/abort
separately. Original gates remain: membership-only p50 at most 50% of B0 with
gain beyond both IQRs; mixed/ordinary/setup p50 no worse than max(5%, 0.5 µs),
p95 max(10%, 2 µs); small bulk p50 max(25%, 2 µs); small append allocation
at most 256 additional sampled bytes/action. Report all failed/noisy cells.
The original 65% large allocation reduction, lifetime allocation, live/postdrop
and 100-cycle churn requirements are preserved but not newly established by
this scope. Stop on correctness failure, an attributable clear regression or
the ceiling; unresolved tail differences are not dismissed or used to expand
the run. A repeatable benefit supports only this direct design, not adoption.

**Budget.** One run, eight minutes maximum including smoke, calibration, both
runtimes and profiles, no deadline reset or broad confirmation. Estimated
3–6 minutes from prior setup/large-action costs; absent cells remain incomplete.
Offline browser routing, no network/dependencies or other packages; coordinator
owns browser/process cleanup and saves partial evidence at every block.

**Proposed retention clarification, not approved.** Preserve the original
whole-heap failed/unestablished result and its 16 KiB number. For future decisions,
report (1) retained application graph and (2) persistent module/runtime footprint
separately. Apply the existing live-overhead max(10%, 16 KiB) and postdrop 16 KiB/
100-cycle no-growth criteria to attributable application-graph storage, with
matched baseline/candidate snapshots and retaining roots. Records, incarnations,
subscriptions, callbacks/selected values, journals/queues, maps and escaped plan
scratch belong to (1); after abort plus caller drop, reachable measured product
objects fail release even when their bytes fit. Caller-held snapshots are a
separate declared owner. Shared compiled code/feedback, runtime templates/shapes
and browser/probe metadata belong to (2), never silently subtracted or declared
free. Measure/report their absolute and candidate-minus-control footprint and
churn slope; unexplained growth or ambiguous attribution is inconclusive.
No numerical code-footprint acceptance budget is invented here. This proposal
cannot convert the old failure into a pass or establish full memory acceptance.

#### Proposed reduced continuation, awaiting agreement (2026-10-26 UTC)

This short proposal was not approved. The complete constructor-audit amendment
below supersedes it; no continuation ran under this summary or its deadline.

The initial run is stopped and preserved after 121.260 seconds. Its first
large-append calibration costs 9.202 s control versus 28.676 ms candidate;
the subsequent warmup pair costs 41.473 s versus 147.783 ms. Fifteen measured
pairs plus warmups cannot fit the eight-minute ceiling. Do not silently change
sample counts. The new proposal keeps all partial setup results, prioritizes
C01/C02/C07 and five Chromium C02 allocation profiles, then takes only two
diagnostic pairs for each B02/B04/B06/B09 in Node and Chromium. Large pairs
provide no p95 or adoption claim. Keep original numerical gates and label
unfinished cells unmeasured. No additional variants or repeated calibration.
Continuation ceiling is four minutes or the original run's absolute deadline,
whichever comes first; no reset of the eight-minute deadline. The owner must
agree this sampling amendment before it executes.

### Stage 6: constructor audit and complete reduced continuation proposal (2026-10-26 UTC)

**Owner approval, recorded before execution.** The owner approves the five-minute
continuation as a diagnostic only. Fifteen-block p95 is effectively the maximum
and cannot independently establish a tail regression; forty-block cells also
provide limited tail evidence. These qualifications govern interpretation of
all numerical allowances below. The deadline takes precedence over completing
the matrix: preserve unfinished cells as unmeasured, with no automatic
continuation. Answer whether the matched construction difference repeats,
what sampled allocations the direct candidate adds, and whether D0 separates
method/property costs from larger-code effects. Adoption stays separate.

**Status: reported diagnostic; completed in 95.401 seconds.** The owner
requests exact constructor differences, equivalent fixtures/warmup/boundaries
and a complete reduced continuation. This supersedes the preceding summary.
It preserves every historical result, original failed/unestablished gate and
source revision. The original eight-minute run's absolute deadline has expired;
the proposal below requests one new bounded continuation, not a reset or a
claim that the old run completed.

#### Exact constructor change and source allocation inventory

Control SHA-256 `1cbd0b38c8a9274c00c5a8f2692319ce84886a659ff69eef5786510fd5b96907`;
direct candidate `11448bf7cb9391b3d7751478a7df4006c912aa4f8db5ed0d9844176e789fb0c6`.
Offline syntax analysis removes only the candidate's nested `updateMembership`
declaration and its returned-object property, strips comments, then prints both
factory bodies. Both canonical bodies have SHA-256
`d5e1b7d043137524c619e4174db2cb72f2df68eb2ba54eb7327c10eddbfbb2fa`.
The `incarnate` and `toItemRecord` helpers also match. This is static source
analysis, not a constructor execution, allocation profile or performance check.

The only factory-body changes are one nested function declaration and the
`updateMembership,` shorthand property between `update,` and `insert,` in the
returned object. The method guards, sets `writing`, calls the module planner
in `try/finally`, then applies its returned plan through the original map/log/
commit operations. None of that body executes during construction. At module
scope, B1 additionally defines the planner; both reporter imports resolve to
the same shipping `src/report.js`. Typedef/comments and import relocation for
the ignored variant do not add constructor execution.

| Source-level initialization/allocation                                | Control                                                                                         | Direct candidate                 |
| --------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- | -------------------------------- |
| Input key checks, Map lookup/set, record copy/freeze                  | N identical iterations                                                                          | Same                             |
| Incarnations / copied record objects / item subscription arrays       | N / N / N                                                                                       | Same                             |
| `Object.getOwnPropertySymbols` result arrays while validating records | N temporary arrays                                                                              | Same                             |
| Collection Map                                                        | 1, initially empty then populated                                                               | Same                             |
| Outer array literals                                                  | 14                                                                                              | Same 14                          |
| Frozen view object and returned collection object                     | 1 each                                                                                          | Same, plus one returned property |
| Named function declarations directly in the factory                   | 24                                                                                              | 25, adding `updateMembership`    |
| Inline view functions/accessors and returned size accessor            | 5 function sites                                                                                | Same 5                           |
| Scalar initialization                                                 | `version`, `queued`, `depth`, `batchId` = 0; `delivering`, `writing` = false; `baseKeys = NONE` | Same                             |
| Omitted options argument                                              | Default empty options object                                                                    | Same                             |
| Bulk scratch / returned plan / bulk sets/maps                         | None during construction                                                                        | None during construction         |

The fourteen arrays are `initial`, `membership`, `wide`, the six `queued*`
arrays, `deferred`, `log`, `touched`, `inserted`, `removed`. `initial` becomes
the frozen keys; it is not copied again on construction. Map backing storage,
array capacities, iteration temporaries, closure contexts, object shapes and
VM allocation elision are engine-dependent and are not exact byte counts from
this inventory. The extra function/property can affect these layouts and
factory compilation. The former candidate's 32-byte shallow public-method
closure is observed heap evidence for that former revision, not a measured
allocation total for the new direct candidate. Direct constructor allocation
bytes have not been profiled.

#### Fixture, warmup and boundary audit of the stopped run

Logical inputs match: fresh `entries(N)` with keys `t0…`, records `{ label:
Task i, done: i % 3 === 0 }`, no subscriptions for constructor-only cells.
Each constructor block allocates its entries and result-reference array before
the clock, then times the factory calls plus storing their returned references.
All constructed collections stay live until after the clock. Size/frozen-keys
checks and release are outside. The subscribed cell times the same full
fixture setup in both variants, including inputs, owner and N+2 subscriptions;
abort/checks are separate. These are two different named setup boundaries.

The boundary code is identical, but live heaps and warmup work are not matched:

| Cell                   | B0/B1 constructions per block | B0/B1 input items and retained result items per block | Measured blocks below 20 ms, B0/B1 |
| ---------------------- | ----------------------------- | ----------------------------------------------------- | ---------------------------------- |
| Constructor N=10       | 1,024 / 2,048                 | 10,240 / 20,480 each                                  | 14/15 / 6/15                       |
| Constructor N=1,000    | 64 / 8                        | 64,000 / 8,000 each                                   | 5/15 / 15/15                       |
| Subscribed setup N=100 | 64 / 64, sequential lifetimes | Same sequential fixture sizes                         | 13/15 / 13/15                      |

There are three warmup blocks and fifteen measured blocks for each variant,
but warmup uses those same unequal counts. Calibration always runs B0 first;
timed blocks alternate first variant. Both share one Node process/heap after
semantic/preflight workloads, with different allocation histories from each
implementation. There is no fresh-process confirmation or forced-GC boundary.

**Runner defect:** calibration stops when the sum of trials with changing
counts reaches 20 ms, then chooses only the last count. It neither requires
that final trial to reach 20 ms nor enforces 20 ms in each measured block.
This conflicts with the original minimum accumulated time per block. Keep all
raw samples and observed failed allowances, but classify this comparison as
measurement-limited; neither its unfavorable N=10 result nor favorable N=1,000
result establishes the true constructor ratio. The prior candidate rejection
is unchanged. The unequal retained chunks double N=10 live fixture pressure
in B1 and reduce it eightfold at N=1,000; normalization does not repair GC or
block-tail effects. This is a concrete confound, not a demonstrated complete
cause. An extra unused method may also affect closure/property allocation,
shape/backing storage and factory compilation. No profile currently attributes
the 5.48 µs median gap to any one of those mechanisms.

#### Proposed continuation: one candidate, fixed diagnostic control

B0/B1 remain the exact hashes above. Add D0 only as a constructor diagnostic:
regular source with a single unused one-argument `updateMembership` function
closing over existing `guard`/`items`, and a property in the same position as
B1. Its body is `guard(); return items.has(changes);` and is never invoked.
It is not a membership implementation, is never used in action cells, and
cannot be adopted. Its prepared, unexecuted source SHA-256 is
`e1b58a4355ca84ba2af7ad093c8b2e36c5c9b7ae7410bcb53eb94e773c23badb`;
its canonical factory body also matches after removing the method/property.
This isolates
the closure/property family of costs from the larger actual method body and
module planner. Module imports/factory code sites still differ; D0 is evidence
for attribution, not proof of identical machine code or a substitute candidate.

All code remains ignored lab code; shipping sources/exports stay unchanged.
Use the existing Node and pinned Chromium, record exact versions, source and
host load. Offline browser routing only; no network, dependencies or other
package work. No concurrent owned check/profiler campaign. Other-session load
is reported; no session is stopped to improve a result.

| Phase/cell                                                                | Variants, fixture and repetitions                                                                                                                                | Wall ceiling                                   |
| ------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------- |
| A: constructor N=10                                                       | B0/B1/D0; three independent Node processes and three fresh Chromium realms; 3 warmup + 40 measured blocks per variant in each                                    | 100 s for all A cells including imports/smoke  |
| A: constructor N=1,000                                                    | B0/B1; same replicas; 3 warmup + 15 measured blocks each                                                                                                         | Included in A                                  |
| A: subscribed setup N=100                                                 | B0/B1; same replicas; 3 warmup + 15 measured blocks each                                                                                                         | Included in A                                  |
| B: C01 empty bulk, C02 single append, C07 ordinary item update, N=10      | B0/B1; one fresh Node process and one fresh Chromium realm; one restored fixture per variant/cell; 3 warmup + 15 measured blocks each per cell/runtime           | 45 s                                           |
| C: constructor allocations N=10 and N=1,000                               | B0/B1; Chromium, 5 independent profiles each; 256 and 4 constructions/profile respectively                                                                       | 65 s for all C work                            |
| C: C02 append allocations N=10                                            | B0/B1; Chromium, 5 independent profiles each, 1,000 fresh independent actions/profile                                                                            | Included in C                                  |
| C: constructor CPU/GC attribution N=10                                    | B0/B1/D0; one separate Node diagnostic trace each, 1 block of accumulated 20 ms; never mixed into primary timings                                                | Included in C                                  |
| D: append, scattered removal, replacement N=C=1,000; mixed N=1,000, C=100 | B0/B1, one fresh checked action each per workload per runtime; exactly 8 control calls total across Node/Chromium, no extra large calibration/warmups/repetition | 30 s overall; 3 s external watchdog per action |

The old N=10,000 timing cells remain incomplete; the new N=1,000 diagnostics
do not fill them or establish scale equivalence. Use the same seed-1 generator,
stable anchors, canonical input conversion, item/subscription shape and full
order/value/identity oracle. Large D actions are also their counted/checked
smoke: validate/count in separate small-N smoke beforehand; D timing contains
no counters, and full checks follow its clock. There is no second large smoke
action. If an action times out, report timeout rather than latency and do not
retry it or enlarge its watchdog. D is diagnostic regardless of setup outcome;
it cannot override a failed gate. No p95 or 2× acceptance claim from one pair.

**Matched chunk and warmup mechanics.** Constructor chunk sizes are fixed at
256 collections for N=10 and four for N=1,000, identically for B0/B1/D0.
Subscribed setup uses eight identical sequential lifetimes per chunk. Prepare
entries/result holders outside constructor clocks; retain only that chunk's
collections, validate and drop it before another chunk. Do not retain all
results from a whole block. Allocation profiles use those same fixed chunk
sizes. No primary forced GC. Keep at most 100,000 live prepared items,
including inserted records and any oracle holdings that keep product records.

For every warmup and measured A/B block independently, accumulate timed chunks
only until that block reaches 20 ms; retain chunk counts/time and stop at the
first crossing. Never substitute cumulative calibration time across counts.
The chunk size, inputs and maximum live heap are equal; block repetition counts
may differ as needed to reach 20 ms. Record first-use imports/setup separately
from warmed primary distributions. Cap cheap/single-change blocks at 100,000
calls, other blocks at 10,000; cap each block at two wall seconds. A block that
cannot reach the minimum is unresolved. Keep it, stop that cell, and do not
recalibrate or enlarge it. Small-action chunks contain 64 separately timed
actions on one restored fixture per variant/cell, with full checks and the
same public-operation reset outside each action clock. C02 reset removes its
appended key, C07 restores its original record values, C01 changes nothing;
the existing reset then verifies original order and refreshes baseline record/
keys references. Initial item subscriptions and initial values are equivalent;
these cases remove no original subscribed incarnation. Capture setup and final
abort separately; never accumulate fresh subscribed lifetimes to obtain 20 ms
of empty-call time. Time full logical action/input conversion/delivery and
retain individual chunk/action counts; check every post-action state outside
timing. Record 1,000 empty timer-pair observations as a separate diagnostic
before B, never subtract them or use them as product times. There are no counters
or profilers inside A/B primary clocks.

Each replica begins with identical small fixture smoke, one check per variant
and family; no 10,000-item workloads precondition its constructor heap. All
module imports finish before warming. D0 has no bulk smoke. Rotate/permutate
variant block order (B0/B1 for two, all six orders cyclically for three) and
alternate the first order across fresh replicas. Warmup chunk size/duration
rule and counts of blocks match. Report results by process/realm and cell;
never pool away a failing replica. p95 means nearest-rank p95 of block averages,
not individual setup latency; forty samples' p95 remains an estimate.

**Allocation/trace boundaries.** Use 512-byte Chromium sampling, including
minor/major-collected objects. Constructor inputs/result-holder arrays are
prepared before profiler start; sample factory calls/storing results only.
For C02 prepare 1,000 independent N=10 fixtures before start, profile exactly
one complete action each, stop sampling, then validate/abort/drop. Record raw
profiles and sampled bytes/call with attribution, never exact allocated bytes.
Alternate first variant each of five profile rounds; one untimed equivalent
warmup chunk precedes profiling. Runtime/CDP/probe allocations stay visible and
identified, not silently removed. D0 needs no allocation acceptance profile;
the CPU/GC trace and its primary setup values are diagnostic. Node traces
capture GC events and CPU samples in the same diagnostic span, flush and close
observers/sessions afterwards, and cannot replace uninstrumented timings.

**Decision criteria, unchanged unless explicitly marked diagnostic.**

- Every semantic/value/order/identity/rollback/cleanup probe must hold;
  failure stops/rejects the candidate, preserving the failing fixture.
- Primary setup/ordinary p50 allowance is max(5%, 0.5 µs); p95 max(10%, 2 µs).
  Small bulk p50 allowance remains max(25%, 2 µs), p95 max(10%, 2 µs).
  A failing replica is not averaged into passing; unresolved timing/minimum or
  conflicting replicas cannot establish non-regression. Report IQR, absolute
  deltas and all raw block counts. The diagnostic 40-block setup cell does not
  retrospectively replace or pass an old failed observation.
- Constructor profiles explain component allocation, with no new acceptance
  threshold. C02 retains the additional 256 sampled bytes/action limit. Five
  paired profiles report median/range; variation straddling that limit or
  inadequate sampling yields inconclusive, with no additional profiles.
- If B1 and D0 reproduce a setup difference against B0, inspect allocation/
  CPU/GC evidence for a closure/property-related cost. If only B1 does, inspect
  actual method/factory compilation or code-layout effects. These are tests of
  hypotheses, not predetermined causal conclusions; absence of attribution is
  explicitly unresolved. Do not construct a reorder bridge from these results.
- D reports checked raw times and ratios only. Membership-only 2× speedup/65%
  allocation reduction, mixed non-regression, lifetime allocation, live/postdrop
  retention and 100-cycle churn gates remain unchanged and are not established
  by these few diagnostic pairs. No fresh large allocation/retention or broad
  confirmation phase. Original whole-heap failure and proposed clarification
  remain separate; no application/code-byte categories are silently dropped.
- A repeated setup failure keeps adoption blocked. Continue only the named
  bounded attribution/small-cost/one-pair diagnostics, even if setup fails;
  their benefits cannot waive that failure. An unresolved result stays
  inconclusive; there is no extension or automatic strategy change.

**Total authorization sought.** One new continuation of at most **300 seconds**
from its owned coordinator start, inclusive of preparation, all phases, imports,
smoke, instrumentation and cleanup. The phase ceilings above total 240 seconds;
the other 60 seconds is global preparation/cleanup headroom, not an extension
to any phase. Stop/preserve partial evidence at any deadline. After the earlier
121.260-second stopped run, this requests at most 421.260 seconds of combined
run wall time, below the former eight-minute aggregate allowance; it does not
pretend the expired original absolute deadline is still running. Approval must
explicitly authorize this new continuation. Estimate 2–4 minutes from the small
setup timings and N=1,000's roughly hundredfold lower order-copying work than
N=10,000; that scaling estimate is not a timing claim. If caps prevent adequate
resolution, report inconclusive rather than spending the full allowance on
repeated expensive controls. No new run/check has been started for this plan.

### Stage 6: integrated final candidate and bounded acceptance proposal (2026-10-26 UTC)

**Status: implemented for review; bounded acceptance approved (2026-10-26 UTC).**
The owner authorizes V → M → T → report without further approval inside the
recorded phase ceilings and single 780-second deadline. Original numerical gates
and evidence remain intact. The application-retention interpretation below is
explicitly approved: persistent code/runtime footprint is reported separately,
and application objects retained through code still fail release. Cap-limited
cells remain unresolved unless conservative bounds establish compliance.
Conflicting setup replicas are insufficient acceptance evidence, not proof of
an intrinsic regression. Heap/profile counts are maxima, never extensions.
The reviewed integrated changes remain intact for the owner's final decision,
including if the recommendation is rejection; no automatic discard is authorized.
This approval precedes every new acceptance observation and covers both timing
method clarifications below.

#### Candidate and evidence boundary

The regular `createCollection` now includes the detached `updateMembership`
method and module-level `planMembership`. There is no experimental export,
second runtime, strategy switch, receiver protocol or registry. The supported
status remains pending; public guides/examples are not updated to advertise
acceptance before the decision. Its source SHA-256 is
`a34241150895ac77b61cc90094851c8657fd2b0790a7c21ca7ee72c988634c3f`.
The exact pre-bulk control `1cbd0b38c8a9274c00c5a8f2692319ce84886a659ff69eef5786510fd5b96907`
and measured prototype `11448bf7cb9391b3d7751478a7df4006c912aa4f8db5ed0d9844176e789fb0c6`
are preserved, including the delivered cleanup fixes. Reversing the recorded
prototype additions reconstructs the control byte-for-byte, verified by that
hash; the source is frozen before acceptance, not read from a moving checkout.

Offline parsing confirms the entire executable module has the same canonical
syntax as the measured direct prototype after stripping JSDoc/comments and
normalizing the equivalent reporter import. Canonical runtime hash:
`211a9037a90014846aa82788f0cd8556e4bb063c0e460257c53bdfa3256bdd7c`.
The integration changes formatting and public/private JSDoc types, restores
the factory's own generic documentation, and uses its normal reporter import;
no executable planner, application or ordinary-operation statement differs.

Reuse prior evidence for algorithmic work, correctness scenarios, allocation
attribution and selection of counts/caps. Preserve all prior measurements and
the matched diagnostic's conclusions; do not repeat D0, constructor allocation
sampling, CPU/GC attribution or 10,000-item copying diagnostics. The physical
source hash/package path has changed, so prior timings/profiles are not renamed
as measurements of the integrated file. Final acceptance samples use the actual
`@canonic2/lattice/collection` entry and the frozen final source hash; consumer
acceptance uses the packed archive. Source/path confirmation below is deliberately
small rather than restarting the original matrix.

#### Correctness and lifetime review

| Boundary           | Source finding                                                                                                                                                                           | Authored acceptance coverage                                                                                                                                                                                          |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Planning           | Existing guard encloses all input/getter reads; `finally` restores writing. No mutation before a complete plan; empty input allocates no plan buffers.                                   | Read-once fields/tuples/record getters, invalid arrays/keys/records/anchors, errors after a copied gain, caught failures preserving earlier outer writes, function-patch guard and independent cross-collection calls |
| Order/identity     | One original-order walk and O(C) bucket traversal, one frozen order, stable anchor groups; survivor records are untouched and gains are copied incarnations.                             | Detached calls, retained keys, stable groups/append, copied records and same-key replacement                                                                                                                          |
| Batch/read         | One saved keys entry plus O(C) entry/removal/insertion undo operands. Ordinary writes retain their existing immediate reads; a bulk call publishes its complete new state synchronously. | Ordinary/bulk reads in one batch, exact nested rollback, thenable rollback and silent transient membership                                                                                                            |
| Subscription owner | Old incarnation subscriptions survive pending removal and end at commit; an explicit abort remains ended across rollback. No transfer to a reused key.                                   | Native listener detach, no redundant abort cleanup, abort during failed planning/rollback, replacement watcher isolation in Node/Chromium                                                                             |
| Delivery           | The existing versioned queue owns commit operands until drain. There is no new scheduler; guard clears before callbacks can reenter.                                                     | Reentrant membership commits, previous keys, late registrations, callback/report errors and subsequent delivery                                                                                                       |
| Release            | Scratch/plan are call-owned only. Existing journal/reset and queue/drain clear their operands; the extra method has the existing collection context as its owner.                        | Weak references to obsolete records/keys, discarded rollback gains, aborted selections and the collection after abort plus caller drop                                                                                |

These are static findings and authored tests, not unexecuted test-pass claims.
No source-level new persistent owner or changed ordinary operation was found.
Actual retained objects still require the package-path graph measurements below.

#### Proposed one-run scope and ceilings

Request **one 13-minute acceptance stage**, with no automatic continuation.
Each phase has its own ceiling, all charged to the same 780-second coordinator
deadline, including fixtures, smoke, observations, artifacts and cleanup:

Execute **V → M → T**, with fresh timing processes after memory work. Within
T, do the short representative bulk/small confirmations before the setup
replicas. This prevents the historically noisy setup comparison from starving
the requested lifecycle evidence; clear rejection still stops unnecessary
remaining work. Phase names below identify budgets, not a different order.

| Phase          | Fixed scope                                                                                                                                      | Ceiling                                     |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------- |
| V              | Affected Node correctness/import/packed consumers, focused Chromium behavior, JSDoc checking; commands below, separately and before measurements | 180 s                                       |
| T              | Three-replica setup comparison, representative bulk package-path comparison and small controls in Node/Chromium                                  | 300 s                                       |
| M              | Calibrated attributable heap/release/churn plus only remaining action/lifetime allocation confirmations                                          | 240 s                                       |
| Report/cleanup | Offline summaries, retaining-root review, exact source verification and owned-resource cleanup                                                   | Remaining 60 s; never extends another phase |

Estimate 8–11 minutes: the matched setup phase took 53.6 s with fewer blocks;
the new setup counts imply roughly 160–180 s including preparation, small
controls roughly 60–90 s, and bulk controls at N=1,000 cost tens of milliseconds,
not the 9–41 seconds at N=10,000. Earlier 24 full-replacement Chromium heaps
at N=10,000 took 194 s; the lower-cardinality capture below has a 240 s memory
ceiling and must stop if calibration/capture costs prevent completion. These
are estimates, not renewed allocations when a phase exhausts its budget.

Keep the 100,000-live-item preparation ceiling, bounded matched chunks and
existing numerical performance/allocation limits. No network, dependency
changes, Workbench work or other owned heavy process runs beside measurement.
Record host/runtime versions, load, all source hashes and archive contents.
Freeze the checked source before T/M; no edits or engine tuning during samples.
If a source changes, preserve its results and invalidate only affected evidence;
there is no deadline reset or full-matrix rerun.

#### V: affected checks

Run these documented categories separately, not concurrently with measurements:

```sh
pnpm --dir packages/lattice test src/modules/collection/collection.test.js src/modules/collection/bulk-membership.test.js tests/import.test.js tests/package.test.js
pnpm --dir packages/lattice run test:browser src/modules/collection/bulk-membership.browser.test.js
pnpm --dir packages/lattice run typecheck
```

The package test packs source into an isolated consumer and covers the ordinary
entry, detached membership, replacement watcher isolation, generic inference,
readonly readers/tuples, invalid records/keys/anchors/tuples, and Node/browser
type environments. The separate experimental import remains refused. Existing
TypeScript consumption settings are unchanged; generated declarations are a
later workload, not smuggled into this stage. No unrelated suites, lint, audit,
formatting or coverage campaign is requested by this proposal.

Fix necessary fixture/type errors and rerun only the failed affected check
within V's ceiling; do not weaken assertions. A behavioral candidate failure
rejects that revision. Any runtime correction must be separately frozen and
pass its affected correctness checks before remaining T/M samples; prior
invalidated evidence cannot certify it. If that cannot finish inside this
single stage, recommend rejection for this version. No measurement starts
while the relevant correctness/consumer/type checks are failing or incomplete.

#### T: exact cells, repetitions and conflict rule

B0 is the frozen regular control with cleanup fixes. B1 is the one integrated
candidate imported through the ordinary package entry, without extra wrappers
or diagnostic code. Equivalent canonical seed-1 starting states/subscriptions
and API input conversion are used; counters, oracles, resets, fixture/reference
capture and teardown stay outside action clocks. Report setup and teardown
separately. Primary clocks include complete logical action, validation and
commit delivery; timer precision is recorded and never subtracted.

**Setup.** Three independent fresh Node processes and three independently
launched Chromium processes/profiles, paired B0/B1 inside each. Measure constructor
N=10, constructor N=1,000 and subscribed setup N=100. Use **five warmup plus
100 measured blocks per variant/cell/replica**, alternating first variant and
offsetting first order across replicas. Fixed prepared chunks: 256 constructor
instances at N=10; four at N=1,000; eight sequential subscribed lifetimes.
Inputs/result slots precede constructor timers; full fixture construction is
the subscribed timer. Release/check each chunk outside timing; never retain a
whole block's output collections. Each block independently accumulates 20 ms,
stopping at first crossing, with original call caps and a two-wall-second block
ceiling. No force-GC/profiler in primary samples. Short blocks are unresolved.

Report per-replica n/p50/p95/max/IQR and normalized within-round differences;
p95 is of block-average setup times, not individual latency. With 100 blocks
it is no longer the maximum, but remains finite-sample evidence. Apply original
allowances to each replica: p50 max(5%, 0.5 µs), p95 max(10%, 2 µs).
**All three replicas for a cell must fit.** Two failures establish a repeated
observed failure; mixed pass/fail replicas or an unresolved block mean the
setup requirement is inconclusive. Either yields a reject recommendation for
this version, without requiring a VM explanation, pooling away failures,
majority-vote acceptance, a fourth replica or another lab. Earlier diagnostic
observations stay visible; this predeclared final run supplies the decision set.

**Representative bulk.** Append, scattered removal and full same-key replacement
at **N=C=1,000**; mixed item changes/membership/full reorder at **N=1,000, C=100**.
Use two independent sets per runtime, each with one untimed checked warmup
and **seven fresh measured actions per variant/cell**, alternating first variant.
This is 64 B0 membership/mixed calls per runtime including warmups, with no
large calibration loop. Require membership-only p50 at most half B0 in both
sets, gains beyond both IQRs; mixed p50 has the existing max(5%, 0.5 µs)
non-regression allowance. Report observed p95/max under max(10%, 2 µs), with
the small-sample qualification; conflicting sets are inconclusive/reject,
not a reason for extra runs. No N=10,000 timing repetitions are requested.
These are individual fresh-action observations rather than 20 ms setup/small
blocks. This is a second explicit measurement-method clarification for approval:
do not multiply fast candidate calls merely to lengthen a block of already
expensive controls. Report clock resolution/empty-timer observations and assess
the numerical gates conservatively with twice the maximum observed empty-timer
span added to B1 times and subtracted from B0 times. If that uncertainty can
change the gate outcome, the cell is inconclusive/reject; do not recalibrate.
This is an explicit representative-cell amendment: deferred sizes/workloads
remain unmeasured and are not relabeled as passing their historical gates.

**Small controls.** N=10: empty bulk, single bulk append inside a batch, single
bulk append directly, two-item prepend, ordinary empty batch, ordinary insert,
ordinary remove, ordinary item patch and reverse reorder. Also single bulk
removal at N=100. One fresh Node process and Chromium process, **three warmup
plus 15 measured blocks per variant/cell**. Use the existing equivalent restored
fixtures and separately timed 64-action chunks; validate/reset outside each
action timer. Original limits remain: small bulk p50 max(25%, 2 µs), ordinary
p50 max(5%, 0.5 µs), observed p95 max(10%, 2 µs). Fifteen-block p95 is explicitly
exploratory and cannot independently prove a population tail regression;
unresolved/conflicting required evidence produces rejection, not a causal claim.

**One explicit timing-method clarification for approval: the two empty controls only.**
The matched diagnostic showed it can be too cheap to accumulate 20 ms within
100,000 calls. This also applies to the unchanged ordinary empty-batch control.
Time contiguous empty-call chunks with equivalent unchanged
fixtures and check identity/silence outside clocks; stop at 20 ms or the same
100,000-call cap. If still below 20 ms, report an absolute upper bound, not a
relative speedup. Each block's bound is its total wall time, including loop/
clock overhead plus two measured maximum clock ticks, divided by calls.
**Empty bulk requires p50 and p95 of those bounds at most 2 µs; ordinary empty
batch requires p50 at most 0.5 µs and p95 at most 2 µs.** The clock-error bound
must be at most 0.1 µs/call. These prove the existing absolute allowances even
against a zero-cost baseline. Otherwise it is inconclusive/reject.
This changes duration adequacy only for those named empty cases, not numerical
gates, call/preparation caps or the preserved earlier unresolved result. Every
other setup and small-operation block retains the 20 ms requirement. This
exception is covered by the owner's recorded approval above.

#### M: attributable retention and remaining allocation evidence

**Retention interpretation for approval.** Apply live overhead max(10%, 16 KiB)
and post-teardown max 16 KiB/no growth over 100 cycles to **attributable
application storage**, backed by node identities and strong retaining paths.
Also require zero obsolete measured records, keys, incarnation/subscription/
callback/selected-value owners and per-instance contexts after their release
boundary. A tiny surviving obsolete object still fails release even below
16 KiB. An actual application object retained by code/feedback still fails;
root category alone does not excuse it. Keep raw whole-heap observations and
separately report persistent module/code/feedback/shape and browser/probe
footprint, absolute and candidate-minus-control. Do not fabricate a code-byte
gate, silently subtract categories, or require a cause for every VM fluctuation.
The old whole-heap failure remains unchanged. Approval of this plan would adopt
this clarified interpretation only for the final candidate decision.

Calibrate once per runtime, before the main capture: a selected 128 KiB array
is deliberately held through a subscription, then aborted/released. The graph
method must detect its attributable backing storage within 16 KiB and its
release. It must not count a constructor allocation boilerplate as an instance.
Use the corrected string decoding/baseline-ID classifier and ownership roots;
failure/ambiguous attribution stops memory work as inconclusive/reject, with
no extended calibration or extra positive controls.

For Node and Chromium, B0/B1, full replacement committed and rolled back at
**N=C=1,000**, capture five matched states: ten-exact-lifetime warmed empty;
initialized; after replacement/rollback with declared caller snapshots;
aborted with caller snapshots released but collection held; and all caller/
owner/collection references dropped. That is **40 main heaps**. Keep five fixed
GC observations before each, with task boundaries and released inspector groups;
never collect until a favorable number appears. Probe IDs/weak identities cover
original and replacement/discarded cohorts; preserve allowed live/rollback
owners and follow strong roots for every unexpected survivor. Inputs/oracles,
returned debugger handles and old snapshots must not masquerade as framework
retention. Use exclusive/dominated product storage where possible and preserve
shared/caller-owned categories explicitly; ambiguous ownership cannot pass.

Memory cases use fresh variant-only processes/realms, not timing realms or
cached timing oracles. Import only that case's factory before warmup so module
footprint is visible rather than canceled by importing both variants together.
Release input/oracle/caller holds before the states that exclude them; keep
probe metadata scalar or weak and release every temporary inspector group.
Report baseline and later shared-module/runtime categories, including source
strings and shape metadata as well as compiled code, plus whole-heap readings.
This separates collection retention from persistent import/JIT/probe footprint
without pretending their different accounting totals are interchangeable.

Churn uses **N=100, 100 complete lifetimes per transaction outcome**, each B0/B1
in each runtime. Keep scalar counts/weak identities and capture dropped state
at cycle 20 and cycle 100, compared with that outcome's warmed baseline: at
most **16 additional heaps**. Require no accumulating instance/object cohort
and no attributable application-byte growth over baseline/20/100. Code/runtime
footprint and its changes remain reported separately; a new native/code value
without an application owner is not silently called a collection leak.

Only these allocation confirmations are requested, in Chromium with the
existing 512-byte sampler and five independent profiles/variant/cell:

- Action-only small membership: batched single append, direct single append,
  two-item prepend at N=10 and one removal at N=100; 1,000 independently
  prepared actions/profile, all setup/check/reset outside profiling.
- Action-only append/removal/replacement at N=C=1,000; one action/profile,
  sufficiently many allocations for the original 65% reduction gate.
- Complete create/action/abort lifetime for full replacement N=C=1,000 and
  mixed N=1,000/C=100; one lifetime/profile, original +10% allocation allowance.

These are **90 profiles**, not a constructor-allocation or full-matrix reprise.
Alternate first variant per profile round, retain all samples, use the same
logical inputs/timing boundaries and separately identify observer costs.
Require at most +256 additional sampled bytes per small action and at most
35% of B0 allocated bytes for membership-only large actions. Five-profile
variation spanning a required threshold is inconclusive/reject; there are no
automatic extra profiles. Constructor exact added bytes remain a reported
precision limit, not a newly invented acceptance requirement. Empty input has
no planner scratch by code/behavior review; no empty constructor profile is
added merely to chase zero allocations. This profile selection is explicit;
other deferred allocation/lifetime cells are not described as measured.

#### Final recommendation rule

Recommend **accept** only if required correctness/consumer/type checks pass
for the frozen final revision, representative package-path benefits meet their
unchanged numerical gates, small/setup comparisons resolve within limits and
attributable release/live/churn requirements hold under the approved criterion.
Report code/runtime footprint as a cost and retain all unfavorable observations.
Any failed requirement, conflicting replica/set, inadequate measurement or
deadline leaving required evidence incomplete means **recommend reject for
this version**. Do not start another automatic continuation or explain every
VM fluctuation before reaching that decision. A clear earlier rejection stops
remaining measurements rather than spending the rest of the ceiling on a
candidate already rejected by this plan.

This requests evaluation/check approval, not API publication or an adoption
decision. The final report must give accept/reject and its concrete basis;
the regular candidate has no permanent experimental entry or duplicate runtime.
Public guidance/examples change together when the recommendation is accepted.
Leave the reviewed integrated changes intact for the owner's final decision,
whether the recommendation is acceptance or rejection. Preserve unrelated work,
measured revisions, original evidence, research and independent cleanup fixes.

### Host and tools

The same machine and tools as the full-flow lab. Background load is recorded
with each set.

## Conditions

### Product integration source identity

The owner-authorized supported collection source has SHA-256
`63d78b7d8c0a2609adfad9e14f0e641db862eafe7572b2599b91300478f7dce1`.
It includes aggregate integration and caches the validated single inserted key
once rather than repeatedly accessing its operand. The original `f713077…`
measurements remain scoped to that earlier source. No quiet-host/tail result is
inferred for this integrated revision.

### Stage 6: matched five-minute diagnostic (2026-10-26 UTC)

Node v24.12.0 and Chromium 156.0.8078.4, Darwin 24.6.0 x64, Xeon W-2140B,
64 GiB, initial load 10.59/14.72/18.27. The run records public revision
`36443c2d104ed03e764d5737b649c91d1bccf2a6` plus exact source hashes:
control `1cbd0b38c8a9274c00c5a8f2692319ce84886a659ff69eef5786510fd5b96907`,
direct `11448bf7cb9391b3d7751478a7df4006c912aa4f8db5ed0d9844176e789fb0c6`,
D0 `e1b58a4355ca84ba2af7ad093c8b2e36c5c9b7ae7410bcb53eb94e773c23badb`.
All use reporter `e4ad2ac9d60e185067e23107b1133b5f5067dbc16c5b4b9dc41401ce3ad2104d`.
The matched coordinator/probe/Node-host hashes are respectively
`917db30ec7b14bb87613291cce25d1121c3a569696c9efa722dbdead56b705c5`,
`e0f4cacf03d696433eae5020a21ef39ef3a0926c7670803b6c51a9ab0d07a7e9`,
`5fe5a15a1f5c68bce97ffc3b15cdd4979efc70d824696f777afc541b27aec5b9`.

Offline routed, cross-origin-isolated browser fixtures use the same source
unchanged; no network, package-source mutation or restored experimental export.
Fresh Node processes/fresh Chromium realms are separate replicas, not pooled.
Constructor chunks are exactly 256 or four instances at N=10/1,000, subscribed
chunks eight sequential lifetimes, with identical boundaries for each variant.
Every successful warmup/primary block independently crosses 20 ms; source and
measured values are preserved. Primary runs contain no profiler/forced GC.
Sampling/traces use separate spans and retain observer overhead. Shared-host
background load and untraced scheduling/GC remain limitations.

### Stage 6: direct integration focused comparison (2026-10-26 UTC)

Node v24.12.0 on Darwin 24.6.0 x64, Xeon W-2140B, 64 GiB. Initial load
5.98/9.96/13.03; other-session compilation overlapped. B0 source
`1cbd0b38c8a9274c00c5a8f2692319ce84886a659ff69eef5786510fd5b96907`,
direct B1 `11448bf7cb9391b3d7751478a7df4006c912aa4f8db5ed0d9844176e789fb0c6`.
No Chromium phase started. Warm/calibration outputs remain distinct from
primary setup block means. No runtime/package export was edited.

### Stage 6: final accept-or-discard stage (2026-10-26 UTC)

Node 24.12.0 and Chromium 156.0.8078.4 on Darwin 24.6.0 x64, an Intel Xeon
W-2140B at 3.20 GHz, sixteen logical CPUs and 64 GiB RAM. Initial one-minute
host load was 5.21 and independent-group initial load 7.66; host load and
runtime conditioning remain limits. Memory, initial timing, independent
timing and product checks ran sequentially. The package's actual experimental
export was measured read-only against the separately frozen B0. Candidate and
regular-source hashes, report hashes, retained ranges and incomplete scopes
are recorded with the final result. No candidate tuning occurred during
this stage. Profiling and forced GC were absent from action timing windows.

### Stage 6: independent memory phase (2026-10-26 UTC)

Chromium **156.0.8078.4** used the installed Playwright host and a
manifest-derived import map resolving the actual experimental package entry.
No network content, dependency change, bundler or transpilation was used.
Node 24.12.0 coordinated the same macOS x64 Xeon/64 GiB host. Continuation
load averages rose from 7.23/14.89/17.30 to 100.39/58.09/36.79; profiled
phase durations on that busy host are not runtime latency evidence.

B0 remained the frozen measured collection hash
`b2afe023ca3a1ebd81b741e6726273c97ce03a860702496fa70c8db928606f76`;
B1 remained checkpoint hash
`7610e112f6a165f95bbcd13903c92d50e30853729499aca5a884926388405af1`.
Current regular-core fixes have their separate recorded hashes and did not
replace B0. The baseline's recorded public revision is
`02a70ed8d032b83b27e066f51071baca3a0a76b0`; the memory coordinator captured
source hashes rather than a fresh live HEAD. Do not infer a later commit from
the baseline revision. The experimental source did not change during memory.

The original memory coordinator stopped for the Q/C fixture-warming correction;
its exact old source and pre-stop report were preserved. Browser cleanup
completed before stopping its stale CDP wait. The continuation retained the
original deadline, reused 410 valid profiles and collected 390 missing profiles.
Earlier small fresh-action estimates remain exploratory. No counts or gates
changed. Each retained case/variant uses its own warmed realm, then five fresh
product lifetimes per transaction mode with all five GC observations per state.
Warmup exercised ten-item canonical collections, not every retention helper
or large-case branch; one-time probe/runtime/code footprint remains unisolated
in empty-baseline comparisons. Temporary inputs/oracles/references are dropped;
only the explicit caller-hold experiment intentionally keeps old keys.

### Stage 6: checkpoint investigation and admission (2026-10-26 UTC)

All three sets used Node 24.12.0 on macOS 15.6 x64, 16 logical CPUs on
the same Xeon host with 64 GiB memory. B0 loaded the byte-preserved measured
baseline; B1 resolved the actual `@canonic2/lattice/experimental/collection`
package export. Source verification ran before measurements. The ordinary
entry still targets the original source; the prototype was not edited.

Initial checkpoint hash
`c45e032bca979be04c6f285ea0a44a24e81883af902614693a6eeaa45ede0f7d`;
corrected checkpoint hash
`7610e112f6a165f95bbcd13903c92d50e30853729499aca5a884926388405af1`.
The reporter and measured baseline/prototype hashes remain those recorded
above. The manifest hash was
`fb72313f5a16fa52cac9ca79f8e015b26aa96ef221a9392a3772d4492b07e868`.
Both checkpoint versions and the manifest were snapshotted separately.

Each teardown reproduction used three fresh OS processes, both variants in
each process with alternating first position, two warmups and seven measured
single-append blocks. Setup, capture, validation and abort were separate
timers. Profiles and three-pair ordinary/pre-GC controls ran afterwards in
separate fresh processes, excluded from primary timings. Initial reproduction
load averages were 7.82/9.04/9.93 before and 17.49/13.53/11.63 after;
corrected reproduction 13.88/13.08/11.58 before and 10.36/14.04/12.63 after.
No timing outliers were excluded.

The corrected checkpoint admission used the same twenty-case driver and
five-warmup/fifteen-measured-block protocol, with separate semantic/counting
validation and uninstrumented action clocks. Q01/Q02 both used 1,000 matched
actions per block; cold samples used five/100 lifetimes respectively.
It recorded public revision `02a70ed8d032b83b27e066f51071baca3a0a76b0`,
load 10.13/13.04/12.34 before and 21.39/15.55/13.32 after. The admission
coordinator hash was
`07b65e34533b60078e78f43b40adc98e3bb0e2506e6fe0421666e1aba3e54990`;
the package composition adapter was
`8bfdb56880093552a74ba908ddea712894c7428ccfac46c2cbe6fc533c1f5d25`.
Chromium was not launched because the Node setup gate stopped admission.

### Stage 6: eight-minute diagnostic amendment (2026-10-26 UTC)

Node 24.12.0, macOS 15.6 x64, 16 logical CPUs on a 3.20 GHz Intel Xeon
W-2140B with 64 GiB memory. Power state was unavailable. Load averages were
60.02/23.87/15.53 before and 11.51/17.17/14.66 after; initial host contention
was substantial. No observer or profiler ran in the primary setup or bulk
measurements. The clock probe observed a minimum positive increment of
0.115 µs. Raw blocks and separate instrumented traces were saved locally.

Public revision `02a70ed8d032b83b27e066f51071baca3a0a76b0`; shipping
collection, reporter and candidate hashes exactly matched those recorded for
the reduced run. No measured implementation changed. The new fresh-process
endpoint hash was
`b500816add8c1ad4d7f9e664a674d99a9e0e2b00641b091da10c99977c4bb635`;
the coordinator hash was
`e7bcf2f5cc524059a92c63c76cd9d3a0dcb91d7b60def4c609d05aa8a0f07633`.

The owner clarified within-process interleaving before measurements. Three
fresh OS processes each ran both variants, strictly alternating the first
variant across paired blocks; process 2 started with B1, processes 1/3 with
B0. Each variant had 20 warmups then 200 measured blocks of 100 fresh
setup/removal/abort lifetimes. Setup timing excluded reference capture,
removal, abort, correctness and IPC; teardown timed abort separately.
No setup process was pooled with another. Instrumented attribution used two
additional fresh processes, 60 blocks of 100 lifetimes per variant; these
blocks are excluded from the primary results.

Bulk used a new fresh process per workload, interleaving both variants and
alternating which ran first. Each variant had two warmups then seven measured
single-action blocks. Fixture creation, reference capture, validation and
abort stayed outside action timing. API input conversion and commit cleanup/
delivery stayed inside. The helper's capture-contaminated lifetime span was
not used: only separately bounded setup, action and teardown were reported.
There was no calibration, forced GC, allocation sampling, matrix expansion,
Chromium run or independent confirmation.

### Stage 6: approved reduced run (2026-10-26 UTC)

The reduced run used Node 24.12.0 on macOS 15.6, x64, a 16-logical-CPU
3.20 GHz Intel Xeon W-2140B host with 64 GiB memory. Load averages were
9.60/7.20/7.73 before and 7.68/7.13/7.66 after; power state was unavailable.
No browser was launched because the Node gate failure stopped the phase.

The public revision was `02a70ed8d032b83b27e066f51071baca3a0a76b0`.
Measured shipping collection, reporter and local candidate hashes matched the
previous run exactly: collection
`b2afe023ca3a1ebd81b741e6726273c97ce03a860702496fa70c8db928606f76`,
reporter `e4ad2ac9d60e185067e23107b1133b5f5067dbc16c5b4b9dc41401ce3ad2104d`,
candidate `22c3d04f416610f613ad383cd7f7e9738458adbf3ab339d19d69cf293aaeaacf`.
The new shared comparison driver hash was
`c0a61ee6639a384c6b2dd0c6665b32f79462cd4728224e426b339401ca17d029`;
the Node realm dispatcher was
`e52502dbe9ff4af635b37af08b948ef6bde6c230696d48e7cba9b7f3e33eb292`;
the coordinator was
`9918737a2c8947ced4306754e0cec61aa6567a28ca43bc4cb6041c0e27b23763`.

Diagnostics and comparison used separate owned worker realms. Semantic stress
checks ran in a validation realm before each comparison block, never in its
timing realm. Subscriptions in acceptance timings, including reset/rebound
subscriptions, had no counters or metadata histories. Fixtures, reference
capture, exact correctness checks and resets were outside action timestamps.
Cold setup retained the original fresh input-generation/create/subscribe
boundary; post-action validation followed the contiguous lifetime timer.
Q01 had five cold lifetimes per block; Q02/Q03 had 100. Every logical action
started from the same canonical values, order and watcher roles in both variants.

Q01/Q02/Q03 used matched action counts of 1,000/2,000/4,000 respectively,
calibrated once to at least 20 ms within the approved caps. Every case had
five warmups and fifteen measured blocks per variant in seeded paired order.
Individual block durations remain in the raw report; calibration reaching
20 ms does not imply every later block reached 20 ms. Large-case independent
counts were approved but not exercised before the stop.

### Stage 6: resumed evaluation (2026-10-26 UTC)

| Item                    | Conditions                                                                                                                                                                                                                                                                                                                                  |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Amendment               | Recorded before continuation: 100,000 actions only for cheap/single-change calibration; all other caps, live-item limit, performance gates and wall ceilings unchanged                                                                                                                                                                      |
| Small calibration       | Node 01:28:42–01:29:32 UTC; Chromium 01:30:19–01:31:33; all 80 variant cells per engine reached 20 ms                                                                                                                                                                                                                                       |
| Extended calibration    | 01:36:37–02:02:54 UTC; stopped and preserved as a runner correction, 139 paired cells, no main timing cells                                                                                                                                                                                                                                 |
| Corrected initial set   | 02:08:23–02:21:37 UTC; shared the original 01:36:37 start/deadline; watchdog enforced the original 45-minute ceiling without reset                                                                                                                                                                                                          |
| Host and runtimes       | Same Darwin 24.6.0/x86-64, Xeon W-2140B, 16 logical CPUs, 64 GiB; Node 24.12.0; calibration Chromium 156.0.8078.4, headless and cross-origin isolated                                                                                                                                                                                       |
| Background load         | One-minute load about 4.0/5.1 at resumed Node/Chromium calibration starts; 21.4 at corrected initial-set start. Other workspace work was not suspended; no competing benchmark or suite was launched by this evaluation                                                                                                                     |
| Power                   | Unavailable: workspace policy rejected the read-only power-state command. No alternate command was used                                                                                                                                                                                                                                     |
| Source                  | Public revision `ec72064dc3e82a1d2c0490623757b45a488697e7`; control/reporter/candidate hashes unchanged from the earlier smoke table                                                                                                                                                                                                        |
| Corrected driver hashes | Shared harness `ee4108ee2d81970230e700180efdb7b8eddb8e8470872f37c7709736d57c283d`; measurement helpers `664132a7cca64b815cfcaef12c2ce3547aef9b420506108e69e90ced658ba491`; warmed driver `35a37446c8fb09f8fed011d892274a1d76b8c430eb1ea9271096fcee64dd1bc9`; Node runner `b8258619667a909dba1b00ba328994948e7a850f63fcdfa0b4888a9d16e7bb99` |
| Main sampling           | Seed-1 matrix shuffle, paired seeded variant order; same fixed steady-state repetition count for both variants of each cell; five warmups, fifteen measured blocks                                                                                                                                                                          |
| Cold sampling           | Separate fresh create/action/abort samples: 100 lifetimes/block at N <= 100, five at N = 1,000, one at N = 10,000; construction and subscription setup reported separately; validation outside timing                                                                                                                                       |

Both interrupted Node processes ended; completed raw reports remain local.
Chromium calibration closed its browser context/profile process. No external
traffic, downloads or dependency changes were needed. The current driver uses
two warmed collections per cell, at most 40,000 live items for the largest
input/output shape, with reset and validation outside action timing. Fresh
oracle and lifecycle cases are separate; their costs are not excluded from
the reported cold metrics. The prototype stayed unchanged throughout.

### Stage 6: smoke and calibration (2026-10-26 UTC)

| Item                | Conditions                                                                                                                                                                                                                                                   |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Approval            | Owner approved prototype evaluation on 2026-10-25 with the mixed-action amendment recorded before runs                                                                                                                                                       |
| Host                | Darwin 24.6.0, x86-64; Intel Xeon W-2140B, 16 logical CPUs, 64 GiB RAM                                                                                                                                                                                       |
| Tools               | Node 24.12.0; installed Playwright Chromium 156.0.8078.4, headless, cross-origin isolated page                                                                                                                                                               |
| Measured source     | Public revision `ec72064dc3e82a1d2c0490623757b45a488697e7`, plus uncommitted source hashes below; source files served/imported unchanged                                                                                                                     |
| Control SHA-256     | Collection `b2afe023ca3a1ebd81b741e6726273c97ce03a860702496fa70c8db928606f76`; shared reporter `e4ad2ac9d60e185067e23107b1133b5f5067dbc16c5b4b9dc41401ce3ad2104d`                                                                                            |
| Candidate SHA-256   | `22c3d04f416610f613ad383cd7f7e9738458adbf3ab339d19d69cf293aaeaacf`                                                                                                                                                                                           |
| Harness SHA-256     | Shared fixtures/probes `d8286836022ab5535fd20012e79eaac4c8f8d8d805336a32012f21f8488be3e4`; Node runner `56f8c4af2b5c2c8aaf7eb288e7e3d8e65d91a08d2351101b4dac0befbe5aeadf`; browser runner `940f0d87379a1249933d304f05c6b348ba1dee589985a29326efb7377beed006` |
| Run windows         | Node smoke 01:15:23–01:15:44 UTC; Chromium smoke 01:16:34–01:16:53; Node calibration 01:17:03–01:17:04                                                                                                                                                       |
| Background load     | One-minute load approximately 5.1–5.9 in smoke, 4.9 in calibration; other workspace activity was not suspended. No other suite was launched by this lab                                                                                                      |
| State and ownership | Fresh collection fixtures per action, ordinary warm process/module caches; owned browser profile/context, no actual network traffic, no downloads or dependency changes; browser closed after smoke                                                          |
| Calibration         | 1,000-action priming block, then one measured block of at most 10,000 actions, in chunks bounded by 100,000 prepared items; no profiler or forced GC during timing                                                                                           |

Power state was not collected before the early calibration stop. No full
timing or profiling set is described as host-qualified acceptance. The control
and reporter hashes were checked again after these runs and were unchanged.

### Initial comparison (2026-10-25)

| Item            | Value                                                                                                                                                                         |
| --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Dates           | 2026-10-25: timing 00:34–00:37 UTC, counted 00:37, memory 00:37–00:40                                                                                                         |
| Host            | macOS 15, x86-64, Intel Xeon W-2140B with 16 threads, 64 GB memory; Node 24.12.0; Chromium 156.0.8078.4 bundled with Playwright 1.64.1, headless, cross-origin isolated pages |
| Background load | 1-minute load average 27 falling to 11 during the timing set, 11 to 12 for the others: the quietest runs of these labs                                                        |
| Samples         | Timing: 15 pages per variant and size; memory and allocations: 10; counted: 3. No page errored                                                                                |
| Measured source | The store, element helpers and example element as committed with the store's write change, plus the lab's capability, bindings, reconciler and runner, hashed in each report  |

### Stage 3 (2026-10-25)

| Item            | Value                                                                                                                                                                            |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Dates           | Timing 01:16–01:24 UTC, counted 01:24–01:25, memory 01:25–01:33                                                                                                                  |
| Host and tools  | As stage 1                                                                                                                                                                       |
| Background load | 1-minute load average 7 to 9 during timing and counted; memory began at 9 and ended at 45, which affects heap and allocation figures less than timings                           |
| Samples         | Timing: 15 pages per variant and size; memory and allocations: 10; counted: 3. No page errored                                                                                   |
| Measured source | Lattice as committed with bulk compaction, unchanged in both variants, plus the lab's two capabilities, bindings, summary bindings, reconciler and runner, hashed in each report |

### Stage 3 follow-up (2026-10-25)

| Item            | Value                                                                                                                             |
| --------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| Dates           | Timing 01:41–01:43 UTC, counted 01:43, memory 01:43–01:45                                                                         |
| Host and tools  | As stage 1                                                                                                                        |
| Background load | 1-minute load average 49 falling to 28 during timing, the busiest of these runs; 19 to 25 for counted and memory                  |
| Samples         | Timing: 15 pages per variant and size; memory: 10 per variant and size, for variants 1, 1n, 3 and 3n; counted: 3. No page errored |
| Measured source | As stage 3, with `setDoneMany` in both capabilities and the summary binding switchable for the diagnostic variants                |

### Stage 4 (2026-10-25)

| Item            | Value                                                                                                                                                                                              |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Dates           | Timing 02:08–02:21 UTC, counted 02:21, memory 02:22–02:32                                                                                                                                          |
| Host and tools  | As stage 1; the prototype tests ran with the package's Vitest in Node 24.12.0                                                                                                                      |
| Background load | 1-minute load average 9 to 21 during timing and counted; memory began at 15 and ended at 26                                                                                                        |
| Samples         | Timing: 15 pages per variant and size; memory and allocations: 10; counted: 3. No page errored                                                                                                     |
| Measured source | Lattice as committed, unchanged; the lab's collection prototype after the pre-run amendment, its tasks capability and bindings, the stage 3 variants, reconciler and runner, hashed in each report |

### Stage 5 (2026-10-25)

| Item            | Value                                                                                                                                                                                                                                          |
| --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Dates           | Timing 02:50–03:06 UTC, confirmation 03:06–03:16, counted 03:16–03:17, memory 03:17–03:27, garbage collection 03:27–03:35                                                                                                                      |
| Host and tools  | As stage 1; Chromium's trace categories `devtools.timeline`, `v8` and `disabled-by-default-v8.gc` for the collection events                                                                                                                    |
| Background load | 1-minute load average 6 to 9 during the timing set; 9 rising to 67 by the end of the confirmation; 70 to 88 at the start of counted and memory, falling to 13; 12 to 13 during garbage collection                                              |
| Samples         | Timing and confirmation: 15 pages per variant and size; memory and garbage collection: 10; counted: 3. No page errored                                                                                                                         |
| Measured source | The shipping collection as implemented, served unchanged from the package source (`collection.js` SHA-256 `a597d35a…`, `report.js` `e4ad2ac9…`, `store.js` `9bf3b436…`), with the lab's variants, reconciler and runner, hashed in each report |

## Results

### Owner-requested bounded shared-host validation

A short diagnostic used the existing package-candidate/control action probes
under recorded shared-host contention, without changing the original acceptance
gates or pretending an idle slot was reserved. The product candidate hash remained
`f7130777350dde04d1e11bc9bf6e4843418a86161d882b669ca2d0913291dd74`.
One warmup pair and three measured pairs per 1,000-item cell alternated variant
order. Fixture preparation, oracle checks and teardown stayed outside action
clocks. Both probes completed their existing correctness oracles.

| Action            | Control median | Candidate median | Ratio         |
| ----------------- | -------------- | ---------------- | ------------- |
| Append            | 5.486 ms       | 1.192 ms         | 4.60× faster  |
| Scattered removal | 34.073 ms      | 2.343 ms         | 14.54× faster |
| Replacement       | 36.193 ms      | 2.955 ms         | 12.25× faster |
| Mixed             | 7.445 ms       | 0.975 ms         | 7.64× faster  |

The complete diagnostic, including the aggregate cases in their own record,
finished in 31.115 seconds. These Node observations establish a practical bulk
benefit in the tested workload. Three pairs do not establish tails, quiet-host
latency, every ordinary-operation gate or Chromium performance. No acceptance
criterion was retrospectively waived; API acceptance remains a separate decision.

### Stage 6: independent bulk allocation confirmation (2026-10-26 UTC)

The allocation-only command completes 50 profiles across the five predefined
cells in **5.180 seconds** overall (sampling/preparation phase 5.138 seconds),
within the 90-second standalone ceiling. All source guards pass and cleanup
reports no errors. Candidate remains `f713077…`, control `1cbd0b3…`; reporter
and fixture/helper sources match the previous comparison. Raw report SHA-256:
`bac9e75e5f7a85aab0824f8b779d3d3e886bedb289a7152c92671104d2e1799b`.

Node 24.12.0 coordinates Chromium 156.0.8078.4 on Darwin 24.6.0 x64,
16 logical CPUs, 64 GiB. Host load starts 49.57/36.02/33.21 and ends
46.32/35.57/33.07. Those observations qualify the environment, not timing:
there is **no admission attempt or primary timing measurement**. Allocation
sampling includes objects collected by minor/major GC. Every result oracle
and warmup completes, with no test-suite rerun or heap investigation.

Each variant/cell has five profiles, one fresh action or complete lifetime per
profile. Preparation/oracles/action-only teardown remain outside sampling;
lifetime includes construction/action/abort. Nearest-rank p95 equals maximum.
Ratios use cross-profile extrema, not a difference of medians or confidence
interval. Keep all observed variation.

| Cell                           | Control p50 / p95 / IQR bytes    | Candidate p50 / p95 / IQR bytes | Conservative candidate/control ratio | Gate        |
| ------------------------------ | -------------------------------- | ------------------------------- | ------------------------------------ | ----------- |
| Append N=C=1,000               | 15,445,884 / 15,453,720 / 4,552  | 418,332 / 435,680 / 21,272      | 0.026032–0.028226                    | ≤0.35: pass |
| Scattered removal N=C=1,000    | 7,035,488 / 7,072,396 / 40,656   | 239,512 / 248,660 / 12,716      | 0.033065–0.035394                    | ≤0.35: pass |
| Replacement N=C=1,000          | 12,450,176 / 12,458,660 / 9,880  | 659,896 / 666,656 / 7,696       | 0.052063–0.053681                    | ≤0.35: pass |
| Replacement lifetime N=C=1,000 | 13,272,428 / 13,316,772 / 13,672 | 1,468,568 / 1,504,700 / 17,492  | 0.109902–0.113742                    | ≤1.10: pass |
| Mixed lifetime N=1,000 C=100   | 3,394,820 / 3,431,228 / 32,292   | 1,024,168 / 1,050,284 / 17,160  | 0.293556–0.312329                    | ≤1.10: pass |

These pass the allocation gates for the corrected planner; they do not infer
speed from bytes or change the prior failed host-admission result. Only timing
requirements remain. API adoption stays pending and runtime source is intact.

### Stage 6: confirmed quiet-window outcome (2026-10-26 UTC)

The owner confirms the quiet window and authorizes the prepared command through
reporting without another pause. The runner and every guarded source match the
recorded plan at startup and completion. Candidate remains
`f7130777350dde04d1e11bc9bf6e4843418a86161d882b669ca2d0913291dd74`;
control remains `1cbd0b3…`; runner remains `c16b7de…`. No source drift or owned
cleanup failure is reported. Raw report SHA-256:
`7ff8882387af374b1562afab11e281ae634a74e89e9436de78d509ba7898f6b4`;
automated assessment SHA-256:
`b0d0592cc7d8349681c10112225f395d35234005ad36b19779f438d130a29b9b`.

Admission takes 59.951 seconds of its 60-second ceiling. The command completes
automated reporting and cleanup in **60.058 seconds overall**, within the
12-minute ceiling. Eleven five-second windows are observed; none qualifies.
Idle ranges **4.87–31.36%** against a minimum 30%, and one-minute load/logical
CPU ranges **1.099–4.492** against a maximum 0.75. Consequently no eight-window
stable period exists. These host observations fail admission despite the
confirmed scheduling window; the source of load is not investigated or attributed.

The host is Node 24.12.0, Darwin 24.6.0 x64, 16 logical CPUs and 64 GiB RAM.
Chromium is not launched, so there is no new observed browser version or
browser measurement to claim. Aggregate free-memory readings and an initial
paging observation remain preserved with the host data; no heap investigation
or new memory acceptance claim is made from them.

The early-stop rule is followed: **zero target processes, timing actions,
setup replicas and allocation profiles**. Only source guarding, host admission
and arithmetic reporting execute. No passing test or resolved small-allocation
profile is repeated. The automated report's generic bulk-allocation status
(`failed, unresolved or incomplete`) means **unmeasured after failed admission**
in this run, not an observed allocation failure.

| Requirement                                                   | Evidence and recommendation                                                          |
| ------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| Correctness, lifecycle and package consumers                  | Reuse unaffected verification for `f713077…`; no source change or rerun              |
| Framework-owned retention                                     | Satisfied and closed; original corrected decision and raw observations preserved     |
| Direct-append and single-removal allocation blockers          | Resolved by the owner using the exact-revision focused diagnostic; no rerun          |
| Setup timings, Node and Chromium                              | Host-deferred; zero replicas; cannot establish acceptance                            |
| Small-operation timings, Node and Chromium                    | Host-deferred; zero blocks or exploratory tails; cannot establish acceptance         |
| Representative bulk timings, Node and Chromium                | Host-deferred; zero new action samples; older benefits keep their revision scope     |
| Representative corrected-planner bulk allocation confirmation | Unmeasured after admission failure; older bulk profiles do not certify this revision |

**Recommendation: withhold API acceptance.** Missing timing and corrected-planner
bulk allocation confirmation prevent adoption. No attributable implementation
regression is established, and host failure does not reject the implementation.
Keep the integrated method, independent cleanup fixes, tests and all prior
evidence intact. No retry or broader investigation is launched.

### Stage 6: allocation-site correction result (2026-10-26 UTC)

Read-only analysis of the final run's existing profiles finds direct-append
planner self-allocation at 506.924–529.544 bytes/action and caller `perform`
allocation at 115.688–133.540, versus control caller 23.156–35.252. The explicit
request object/tuple shape accounts for real caller work; it is part of the
complete action and remains measured. Planner arrays and plan storage are real
operation overhead. Attribution between `insert`, `incarnate`, `toItemRecord`
and native `push` moves across profiles, consistent with inlining/helper
placement; do not subtract per-function medians to claim exact object sizes.
Single-removal Set construction alone is sampled at 68.248–80.160 bytes/action;
additional Set/table and operand allocation can be charged to the planner.
Saved-site analysis SHA-256:
`82230146d4e9ddf126240a420fd013699b6afe3a5a8eafc5d5729fa7a4ddf160`.

The concrete correction allocates losses/gains and metadata arrays at their
captured lengths and fills them by index, uses shared empty operands for absent
changes, and keeps one removed string in a scalar. Multiple removals retain
the duplicate/lookup Set. All allocation changes are operation-local; no method,
constructor, ordinary operation, subscription or persistent owner is added.
Invalid removed anchors and replacement of surviving keys remain rejected;
removed keys may still be reinserted as new incarnations. The frozen predecessor
source and its original profiles remain preserved. This is an implementation
correction under the owner's focused instruction, not API adoption.

The two-cell diagnostic completes 20 profiles in **7.094 seconds**, within its
60-second ceiling, using Chromium 156.0.8078.4 and the same 512-byte collected-
allocation sampler. Corrected source is
`f7130777350dde04d1e11bc9bf6e4843418a86161d882b669ca2d0913291dd74`;
unchanged control is `1cbd0b3…`. Node 24.12.0/Darwin 24.6.0 x64 coordinates
offline browser measurements. Raw report SHA-256:
`abfcd226ddc198dc8008f4e3b6372ada475b28354363cf33cbc7ea01a907b509`.
An initial preparation stopped before launching a browser or taking samples
because archived control/candidate basenames collided. The archival names were
made unique; that empty attempt and its files remain preserved. No measured
sample was dropped or repeated because of its result.

Each variant/cell has five profiles of 1,000 fresh actions. The result oracle
and teardown run outside sampling; preparation retains at most 100,000 items.
Nearest-rank p95 is the maximum; no statistical confidence interval is claimed.

| Workload             | Control p50 / p95 / IQR bytes  | Corrected p50 / p95 / IQR bytes | Corrected extra-byte envelope | Original envelope    | Result, limit +256 bytes |
| -------------------- | ------------------------------ | ------------------------------- | ----------------------------- | -------------------- | ------------------------ |
| Direct append N=10   | 872.784 / 906.872 / 18.336     | 1,047.936 / 1,065.972 / 12.600  | +93.272 to +195.472           | +249.104 to +358.216 | Pass in this diagnostic  |
| Single removal N=100 | 2,534.976 / 2,546.872 / 34.528 | 2,530.132 / 2,567.880 / 23.572  | −72.032 to +81.084            | +148.912 to +294.276 | Pass in this diagnostic  |

The correction's direct-append planner self-allocation is 353.508–391.992
bytes/action, below the predecessor's 506.924–529.544 range. Single removal has
no sampled Set site, consistent with the removed constructor call. Its planner
self range is 1,111.424–1,297.008 versus the predecessor's 1,322.960–1,379.640;
native helpers may receive other charges. Complete-action bounds, rather than
these partial site totals, decide the two allocation limits. The data supports
a real improvement and closes the former sampling ambiguity for these two
cells under the predeclared conservative bound.

All action result oracles and cleanup complete; no page or cleanup error is
reported. Focused behavioral coverage includes first/middle/last single removal,
read-once inputs, exact rollback identities, deferred subscription cleanup and
key reuse. Affected behavioral/package-consumer and browser verification and
JSDoc checking were rerun because covered source changed, not as acceptance
timing. No setup, primary timing, host admission or heap measurement ran.

The revised planner also changes larger-call buffer allocation. Earlier bulk
profiles remain evidence for `9643138…`, not automatic acceptance for `f713077…`.
No fresh large/broad confirmation is claimed. Retention's ownership decision
remains closed: there is no new persistent owner or evidence reopening it.
Outstanding performance evidence and setup/small-operation timing require an
owner-agreed quiet window with competing workloads paused. Do not start another
admission attempt, matrix expansion or investigation automatically.

### Stage 6: final remaining-requirements outcome (2026-10-26 UTC)

The approved run closed in **193.955 seconds**, including 97.468 seconds of
arithmetic reporting, record reconciliation and cleanup within the 120-second
report allowance and 720-second overall ceiling. It completed collection in
96.487 seconds: allocations 36.439
seconds, host admission 60.003 seconds including scheduling/bookkeeping, and
deferred timing-phase dispatch under 0.01 seconds. No additional admission
window was taken. All 70 permitted allocation profiles completed; no heaps,
retention probes, passing tests or primary timings ran. Owned browser cleanup
reported no errors. Collection source remained `9643138…`; control remained
`1cbd0b3…`. Raw report SHA-256 before assessment:
`99a2d9e8b178940b2ec602606be7d97559439a528866c4070371d990825521ed`.

The host used Node 24.12.0, Chromium 156.0.8078.4, Darwin 24.6.0 x64, 16 logical
CPUs and 64 GiB RAM. Eleven five-second admission observations recorded idle
9.23–60.97% and one-minute load/logical CPU 6.493–9.284, against the unchanged
maximum 0.75. No eight-window quiet period qualified. Consequently no timing
replica, action sample or exploratory 15-block tail exists for this run.
This is host deferral, not evidence of an implementation timing regression.

Allocation summaries below are bytes/action, except complete mixed lifetime.
Each variant has five profiles; nearest-rank p95 equals the maximum. Each small
profile contains 1,000 independent actions, each bulk/lifetime profile one.
Preparation, result checks and action-only teardown remain outside sampling;
complete lifetime includes setup and abort. All warmup and measured result
checks completed. Bounds use the approved cross-profile extrema, not a
confidence interval or a difference of medians.

| Workload                     | Control p50 / p95 / IQR         | Candidate p50 / p95 / IQR      | Conservative envelope      | Gate and outcome |
| ---------------------------- | ------------------------------- | ------------------------------ | -------------------------- | ---------------- |
| Batched append N=10          | 1,697.612 / 1,719.612 / 2.452   | 1,915.756 / 1,935.236 / 40.680 | +163.680 to +249.348 bytes | ≤256: pass       |
| Prepend two N=10             | 2,088.684 / 2,126.724 / 9.428   | 2,272.392 / 2,288.776 / 43.988 | +55.960 to +225.824 bytes  | ≤256: pass       |
| Direct append N=10           | 876.424 / 914.620 / 18.776      | 1,190.004 / 1,219.760 / 17.100 | +249.104 to +358.216 bytes | ≤256: unresolved |
| Remove one N=100             | 2,549.300 / 2,599.900 / 59.884  | 2,749.992 / 2,813.744 / 60.596 | +148.912 to +294.276 bytes | ≤256: unresolved |
| Append N=C=1,000             | 15,445,908 / 15,459,412 / 9,872 | 439,632 / 447,816 / 9,768      | Ratio 0.027704–0.028995    | ≤0.35: pass      |
| Scattered removal N=C=1,000  | 7,016,064 / 7,079,356 / 30,524  | 267,472 / 286,172 / 8,236      | Ratio 0.036436–0.040867    | ≤0.35: pass      |
| Mixed lifetime N=1,000 C=100 | 3,373,048 / 3,404,612 / 28,844  | 1,028,976 / 1,050,800 / 7,680  | Ratio 0.291273–0.312745    | ≤1.10: pass      |

Thus append and removal allocate at least 97.10% and 95.91% less respectively
under these envelopes. Mixed lifetime allocates at least 68.72% less; its gate
remains non-regression, not a bulk-only improvement target. Previously passing
replacement action/lifetime profiles on this exact revision remain valid and
are not repeated. Direct append's median extra is 313.580 bytes; do not hide
that unfavorable observation, but its cross-profile lower bound remains below
256, so the predeclared decision is unresolved rather than established failure.
No additional profiles or attribution investigation were taken.

| Acceptance requirement                                | Final evidence status                                                                           |
| ----------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| Correctness, lifecycle and package consumers          | Reused unchanged-source passing evidence                                                        |
| Framework-owned retention, replacement/rollback/churn | Satisfied; corrected decision remains closed; original byte result preserved                    |
| Direct-append allocations                             | Unresolved: +249.104 to +358.216 bytes against +256 limit                                       |
| Other small allocations                               | Batched append/prepend pass; single removal unresolved: +148.912 to +294.276 bytes against +256 |
| Representative bulk and complete-lifetime allocations | Pass, including reused replacement profiles                                                     |
| Representative bulk timing confirmation               | Deferred-host in Node and Chromium; no new samples                                              |
| Small-operation and setup timings                     | Deferred-host in Node and Chromium; no replicas or tail claims                                  |

### Stage 6: replacement strings and repaired owner checks (2026-10-26 UTC)

The user-authorized focused diagnosis completes **58 captures and 20 allocation
profiles**. Retention takes 59.447 s of its 240 s ceiling; allocation takes
2.604 s of 90 s; the raw coordinator takes 62.082 s. No primary timing, setup,
direct-append matrix or passing test-suite rerun occurs. Report reconciliation
takes 354.119 s, exceeding its separate 150 s allowance;
the recorded overall 416.201 s stays inside the 480 s ceiling. No measurement
or sample count is extended for that reporting overrun. The implementation
remains unchanged at
`9643138df205b2215b48726768b8c72b3cf65eacd0f91a06d462b8ded4544b91`;
control remains `1cbd0b38c8a9274c00c5a8f2692319ce84886a659ff69eef5786510fd5b96907`.
The raw report before assessment has SHA-256
`29dc5dc7a9e5a318e9700501c132793eddfdf6ef70ce2f53f3351d60c846818f`.
Node v24.12.0, Chromium 156.0.8078.4, Darwin 24.6.0 x64, 16 logical CPUs,
64 GiB; load starts 26.86/27.10/32.79 and ends 33.56/29.88/33.50. Contention
is recorded, but this stage makes no timing claim and waits for no host admission.

#### Strings originate in the fixture; copying adds no string owner

B0 and B1 use the same `Object.freeze({ ...item })` record-copy routine.
The detailed template cases already contain **1,000 cons strings/32,000 shallow
wrapper bytes in caller input before the operation**. Immediately after copying,
all 1,000 stored label targets are the same heap objects as the input labels,
in both variants. The candidate did not construct an extra set of label strings.

Before the value oracle, cons strings point to the shared 12-character prefix
and one-, two- or three-character numeric suffixes. The old oracle creates a
new expected string for equality comparison. After that oracle, both variants'
same input/stored cons targets point to a complete 13–15-character flat string
and an empty second child: the payload was flattened while its wrapper remains.
This is observed within individual heaps; no cross-snapshot ID assumption is
used to infer an alias.

Strong paths before input release include the explicit fixture → input → insert
tuple → source record → label; stored labels also have fixture → collection →
method context → items Map → incarnation → copied record → label paths. After
input release, caller source records clear in the WeakRef checks, while the live
record's field value remains. After collection drop all dataset owners clear.
No retained source-record container, planner scratch, journal, old incarnation,
subscription or private context explains the string wrappers. There is no
demonstrated unnecessary implementation retention to correct; string-flattening
logic is not added to the framework's field-copy semantics.

#### Focused repeat preserves both favorable and unfavorable observations

Application bytes below exclude code/shape metadata but retain genuine field
values and their wrappers. They are measured with inputs/snapshots/subscriptions
released and the collection held, N=C=1,000.

| Runtime/input mode            | Detailed set B0 → B1 | Unsampled-interval confirmation B0 → B1 | Gate                                      |
| ----------------------------- | -------------------: | --------------------------------------: | ----------------------------------------- |
| Node template labels          |    278,720 → 276,952 |                       246,720 → 276,952 | Detailed passes; confirmation exceeds 10% |
| Node verified flat labels     |    246,496 → 244,912 |                       246,496 → 244,912 | Both pass                                 |
| Chromium verified flat labels |    155,272 → 154,388 |              Not repeated in this scope | Pass this confirmation                    |

The detailed set explicitly forces GC for an input heap before the action;
both control and candidate retain the same input wrappers afterward. The
independent confirmation preserves the original unsampled prepare/action/oracle
interval. There the control stores flat strings and the candidate retains the
fixture-generated wrappers, reproducing the **+30,232 bytes** against a
24,672-byte allowance. Verified-flat inputs remove that difference without any
runtime source change. Thus fixture-generated string representation, oracle
flattening and GC history account for the captured discrepancy; differential
wrapper elimination is VM/history-sensitive, not evidence of another framework
owner. The exact engine shortcut/tiering mechanism is not independently proven.

The original observation and this reproduced numeric exceedance remain failed
observations under the unchanged application-byte criterion. A passing flat
mode does not silently pass the template mode or adopt the API. These results
refine attribution; they do not authorize dropping strings from the gate.

#### Hidden-owner check repaired and independently exercised

The new check counts strongly reachable logical owners **inside each capture**.
It ignores absent numeric `batch`/`version` edges, recognizes context names with
Chromium scope suffixes, and reads frozen-array backing elements as well as
ordinary element/property edges. It requires known positive controls before
accepting a zero at disposal. IDs are only local graph addresses for printing
retaining paths, never identity keys across captures.

Replay of all eight earlier variant/runtime/outcome sequences now detects
1,000 live incarnations, 1,002 initial subscriptions, one collection context
and the live key array. Committed replacement leaves two subscriptions;
rollback restores 1,002; abort leaves zero. New captures repeat these checks,
also proving source replacement records release after copying. All **58** new
captures pass their declared positive/release checks. At drop and after churn
20/100, dataset records/incarnations/subscriptions/contexts/key arrays and exposed
WeakRefs are zero. This repairs the previous Node hidden-owner proof without
requiring stable snapshot IDs or another runtime implementation.

#### Affected replacement allocation confirmations

Chromium, five 512-byte-sampling profiles per variant/cell, one N=C=1,000 action
or complete create/action/abort lifetime per profile, collected minor/major
allocations included. Input labels are the verified flat fixture; action input
conversion and commit delivery are sampled. Setup/oracles are outside action
profiles; lifetimes include create/action/abort. Every profile is preserved.

| Cell                     |     B0 bytes p50 / p95=max / IQR |  B1 bytes p50 / p95=max / IQR | Conservative B1/B0 envelope | Original gate |
| ------------------------ | -------------------------------: | ----------------------------: | --------------------------- | ------------- |
| Replacement action       | 12,438,644 / 12,553,200 / 55,492 |     698,516 / 702,360 / 3,896 | 5.50–5.67%                  | Pass ≤35%     |
| Create/replacement/abort | 13,219,868 / 13,287,448 / 55,860 | 1,523,112 / 1,539,824 / 4,848 | 11.20–11.68%                | Pass ≤110%    |

Action allocation reduction is at least **94.3%** under the conservative
five-profile bounds. Five-sample p95 is the maximum and is reported as such.
These profiles establish only the two named cells, not direct append or every
membership workload. No further allocation profiles or timing runs are added.

**Outcome:** the owner-check defect is repaired, replacement disposal/churn
checks pass, and both affected allocation gates pass. No attributable
unnecessary framework retention was found, so the runtime is preserved rather
than modified to normalize application strings. General API acceptance remains
unestablished: the unsampled template-mode live-byte gate still fails, timing
is deferred, and unrelated small/allocation requirements retain their earlier
statuses. Preserve all results and keep the implementation intact/unaccepted.

### Stage 6: corrected-candidate continuation outcome (2026-10-26 UTC)

**Recommendation: reject for this version; keep the implementation intact.**
The raw coordinator completes in **179.857 s**: preflight/admission 75.247 s
(90 s ceiling), memory work 104.552 s (240 s ceiling). All **60 maximum heaps**
are captured: four calibration, 40 replacement/rollback, 16 churn. The live-
storage gate triggers the predeclared early stop before allocation/confirmation
phases. There are **zero allocation profiles and zero primary timing/setup
samples**. These are unmeasured/inconclusive requirements, not passing results.
No snapshot/profile count or measurement ceiling was extended. Offline
attribution/reporting **exceeded its separate 150-second allowance**; that is
a process failure, not a renewed measurement budget. Original evidence and
first-pass/attributed results are preserved, including the invalid Node ID
intersections rather than silently deleting them.

The tested candidate remains
`9643138df205b2215b48726768b8c72b3cf65eacd0f91a06d462b8ded4544b91`;
control remains `1cbd0b38c8a9274c00c5a8f2692319ce84886a659ff69eef5786510fd5b96907`.
Reporter SHA-256 is `e4ad2ac9d60e185067e23107b1133b5f5067dbc16c5b4b9dc41401ce3ad2104d`.
The measured coordinator is
`d9bacd1d1af7357ea069f3ea6f2770a21c6537c1c8874483d2b2c8e16a702eaf`;
probe `31a297a3ca073c4fd640caf0c9bc812e0832d599fb83d63e815a25a75a434935`;
graph parser `6247e7b8f644520fdd2f06fb8419279f3770c8f350de89ad57282f6dd6ec440c`.
The raw report before offline assessment has SHA-256
`c94d3586c93cbb6d4dd90599bd12a2609a0b9ff4955958a458e0c0a6aa3e249c`.
Source snapshots and earlier measured sources are preserved locally. No
runtime source changes during execution and no experimental entry are involved.

#### Conditions and host qualification

Node v24.12.0, Chromium 156.0.8078.4, Darwin 24.6.0 x64, Xeon W-2140B,
16 logical CPUs, 64 GiB. Initial load is 18.05/22.83/35.70; final measurement
load 60.12/34.44/37.70. During the twelve admission windows, CPU idle is only
7.49–25.26% against the required 30%; one-minute load per logical CPU is
1.285–1.534 against the 0.75 maximum. No admitted timing period exists.
Primary direct/small/bulk/setup timing is therefore **host-inconclusive**;
there is no intrinsic timing-regression finding from this run. Per the owner's
amendment, all independent retention cases still execute. Allocation work is
subsequently stopped by the live-storage gate, not by host contention.

#### Calibration and lifecycle evidence

The selected byte payload has exactly 131,072 bytes. Node attributes a
137,056-byte held-to-aborted decrease; Chromium 133,348 bytes. Both fit the
unchanged ±16 KiB detection tolerance, and both view and buffer weak identities
clear. Calibration establishes payload attribution/release, not validity of
every cross-snapshot identity operation.

All eight full replacement/rollback cases and their 100-cycle churn sequences
complete. Chromium keeps stable collection IDs across initialized/changed/
aborted-held snapshots. Its four cases have zero obsolete tracked objects at
drop, zero remaining incarnation/record/subscription cohorts at churn 20/100,
and cleared exposed weak identities. Rollback gains and committed original
records/keys clear at their declared boundaries. This establishes Chromium's
checked lifecycle/application-release requirements.

Node's exposed records, keys, methods and signal weak identities also clear
at release/drop and churn 20/100. Its hidden-owner identity evidence remains
**unresolved**: the very same live collection has different snapshot IDs in
successive captures. For example B1 commit reports 70,377 → 122,849 → 121,487;
B0 commit 99,351 → 114,015 → 106,873. Consequently baseline-ID filtering and
ID intersections across that capture path are invalid. Apparent post-drop
survivors occur in both variants and cannot be called retained application
objects. Additionally, the Node incarnation/subscription classifier reports
zero even in known initialized states, so its zero post-drop counts cannot
prove hidden-owner release. This limitation does not affect independent weak
identities or ownership/size traversal within an individual heap. No new heap
or extended calibration is taken to repair it in this stage.

#### Live storage: attribution within individual heaps

The initial guard compares full product-dominated totals after Node commit:
263,904 → 293,000 bytes, then 260,144 → 289,288 after owner abort while the
collection remains held. Offline inspection separates code and shape metadata
from application storage. The application-only comparison **still fails**:

| Runtime/outcome/state                      | B0 application bytes | B1 application bytes | Difference | Allowance | Result                |
| ------------------------------------------ | -------------------: | -------------------: | ---------: | --------: | --------------------- |
| Node commit, initialized                   |              803,656 |              803,680 |        +24 |    80,366 | Pass                  |
| Node commit, replaced                      |              248,576 |              278,776 |    +30,200 |    24,858 | Observed gate failure |
| Node commit, aborted/collection held       |              246,136 |              276,384 |    +30,248 |    24,614 | Observed gate failure |
| Node rollback, initialized                 |              803,616 |              803,680 |        +64 |    80,362 | Pass                  |
| Node rollback, rolled back                 |              827,656 |              803,720 |    −23,936 |    82,766 | Pass                  |
| Node rollback, aborted/collection held     |              259,368 |              235,432 |    −23,936 |    25,937 | Pass                  |
| Chromium commit, initialized               |              477,784 |              477,816 |        +32 |    47,778 | Pass                  |
| Chromium commit, replaced                  |              180,236 |              162,988 |    −17,248 |    18,024 | Pass                  |
| Chromium commit, aborted/collection held   |              175,152 |              157,904 |    −17,248 |    17,515 | Pass                  |
| Chromium rollback, initialized             |              477,784 |              477,816 |        +32 |    47,778 | Pass                  |
| Chromium rollback, rolled back             |              477,856 |              477,836 |        −20 |    47,786 | Pass                  |
| Chromium rollback, aborted/collection held |              129,784 |              129,792 |         +8 |    16,384 | Pass                  |

Within the Node replaced heap, both variants contain 1,000 replacement records.
B0 labels are flat strings; B1 labels retain **1,000 concatenated-string wrappers,
32,000 shallow bytes**, in addition to their string storage. Representative
strong paths run through the explicit fixture's collection → method context →
items Map → incarnation → record → label. These are live record-owned application
values, not module/code footprint or an unreleased caller input. Other category
differences partly offset the wrappers, yielding the +30,200-byte net cost
(about 12.1%) against the 10% allowance. After abort the cost is about 12.3%.

This attributes the captured storage cost to value representation; it does not
establish a repeatable intrinsic planner defect or authorize flattening/changing
records to improve a measurement. Only one matched heap sequence per outcome
was declared. The Node rollback comparison instead favors B1, and Chromium does
not reproduce this excess. Preserve that variation alongside the gate failure.
No numerical budget is relaxed and no application strings are subtracted as
runtime footprint. The predeclared live-storage stop remains justified after
the code/shape separation, so allocation phases are unmeasured rather than
automatically continued on an already failing acceptance candidate.

#### Persistent runtime footprint stays separate

At final drop, Chromium B1-minus-B0 code self sizes are +55,420 bytes for commit
and +52,776 for rollback; shape metadata +472/+508; native totals +10,190/+8,194;
flat strings +372 in each outcome. Whole used-heap differences are +58,544 and
+54,340 bytes. These categories overlap in accounting and are not summed into
an application-retention claim.

Node dropped code differences are +703,976 bytes for commit and +17,416 for
rollback. The commit difference includes a large B0 code-size decrease from its
own warmed baseline, so it cannot establish a constant candidate code cost.
Node native totals are unchanged across variants; raw used-heap ranges and all
baseline/phase categories remain preserved separately. No code/runtime byte
threshold is invented, and the original earlier whole-heap failure is unchanged.

#### Acceptance table and decision

| Requirement                                           | Outcome                                    | Evidence boundary                                                                                           |
| ----------------------------------------------------- | ------------------------------------------ | ----------------------------------------------------------------------------------------------------------- |
| Affected correctness and packed consumers             | Pass, reused                               | Source unchanged; no passing suite rerun                                                                    |
| JSDoc checking                                        | Pass                                       | Corrected revision preflight                                                                                |
| Corrected retention calibration                       | Pass                                       | Both runtimes, detected payload and released view/buffer                                                    |
| Chromium replacement/rollback/application churn       | Pass                                       | Four complete cases; stable identities and release milestones                                               |
| Node exposed weak-reference release/churn             | Pass for those cohorts                     | Record/key/method/signal identities clear                                                                   |
| Node hidden objects/private contexts                  | Unresolved measurement                     | IDs change across captures; classifier misses known live instances                                          |
| Live application storage                              | Observed attributable storage-gate failure | Node committed replacement has extra record-owned string wrappers; intrinsic/repeatable cause unestablished |
| Persistent code/runtime footprint                     | Reported separately                        | Matched totals/category deltas; no added gate                                                               |
| Direct/batched append timing and other small controls | Host-inconclusive                          | Admission failed; zero timing samples                                                                       |
| Setup                                                 | Host-inconclusive                          | No clean replica; no timing regression attribution                                                          |
| Final-revision bulk timing confirmation               | Host-inconclusive                          | No new timing sample; earlier evidence keeps its original scope                                             |
| Small/direct/bulk/lifetime allocations                | Unmeasured                                 | Zero profiles after the predeclared live-storage stop                                                       |

Recommend rejection for this version because a required live-storage observation
exceeds its unchanged allowance and required identity/performance/allocation
evidence remains incomplete. Keep the integrated method, its correctness tests,
original evidence and independent regular cleanup fixes intact and unaccepted
for the owner's final decision. Do not discard code, publish the API, add another
experimental copy or start an automatic continuation.

### Stage 6: integrated candidate final acceptance (2026-10-26 UTC)

**Recommendation: reject for this version.** The approved stage stopped after
190.663 s: checks 25.406 s, memory calibration 1.375 s, timings 163.820 s.
The measured/check phases stayed below their ceilings; the repeated setup-gate stop was applied
without spending the remaining budget. Offline assessment took under one second.
Research/spec reconciliation continued beyond the 60-second reporting allowance;
the raw report and offline assessment were already complete, and no measurement
or calibration was extended. The overall 780-second ceiling was not exhausted.
The integrated implementation and its correctness coverage remain intact for
the owner's final decision. No supported API adoption or automatic continuation
is implied. Original failures, measured revisions and independent cleanup fixes
are preserved.

Node v24.12.0, Chromium 156.0.8078.4, Darwin 24.6.0 x64, Xeon W-2140B,
64 GiB; initial load 33.51/43.12/37.70, final load 275.95/138.34/76.81.
This severe shared-host contention is a major attribution limit; it does not
waive acceptance gates or establish a cause within Lattice. Source/control hashes are exactly the
frozen hashes in the approved plan; no source edits occurred during measurement.
The report before offline assessment has SHA-256
`5f127a5296ad8b8e4882727b1417e5726268d3dfec8e0d1cf42bda84158ac100`.
Fresh variant-only memory processes and separately launched timing/browser
processes were used. No owned measurement resources reported cleanup errors.

#### Representative package-path benefits

Each entry below is **p50 ms/action, B0 → B1**, with seven individual fresh
measured actions plus one checked warmup per variant/set. Membership N=C=1,000;
mixed N=1,000/C=100. All 16 runtime/cell/set comparisons completed, preserving
setup, validation, capture and teardown outside the action clocks.

| Runtime  | Action            | Set 1          | Set 2          | Gate                 |
| -------- | ----------------- | -------------- | -------------- | -------------------- |
| Node     | Append            | 7.674 → 0.834  | 3.713 → 0.742  | Unresolved variation |
| Node     | Scattered removal | 39.603 → 2.212 | 52.503 → 2.416 | Pass both sets       |
| Node     | Full replacement  | 40.806 → 3.452 | 42.098 → 3.352 | Pass both sets       |
| Node     | Mixed             | 8.324 → 0.748  | 8.239 → 0.704  | Pass both sets       |
| Chromium | Append            | 10.675 → 0.710 | 2.710 → 0.510  | Unresolved variation |
| Chromium | Scattered removal | 45.970 → 3.830 | 42.970 → 3.485 | Pass both sets       |
| Chromium | Full replacement  | 56.530 → 5.120 | 34.555 → 3.725 | Pass both sets       |
| Chromium | Mixed             | 8.340 → 1.265  | 9.255 → 0.915  | Pass both sets       |

Removal speedups are 12.0–21.7×, replacement 9.3–12.6×, mixed 6.6–11.7×.
Those cells meet their declared p50, variation and observed tail limits with
conservative timer uncertainty. Append's 5.0–15.0× observed p50 improvement
does not satisfy the additional variation rule: B0 IQRs are 7.691/4.272 ms
in Node and 10.460/3.535 ms in Chromium, larger than the corresponding gains.
It remains unresolved rather than being relabeled a pass. Seven-sample p95
equals the maximum and is exploratory throughout. Deferred larger sizes remain
unmeasured.

#### Small operations and setup

Small cells retain three warmups and 15 measured blocks where complete. Node
passes empty-bulk bounds, batched append, ordinary empty-batch bounds, insert,
remove, item patch, reverse reorder and single bulk removal. Direct append
exceeds the observed limits: p50 2.378 → 6.215 µs, p95/max 61.836 → 581.399 µs;
IQR 2.338 → 13.900 µs prevents a clear attribution claim. Two-item prepend
stops short of 20 ms at its unchanged 10,000-action cap; six B0/five B1 measured
blocks are preserved and the cell is unresolved.

Chromium passes empty-bulk bounds, ordinary insert/item patch and single bulk
removal. Its batched/direct append, ordinary removal and reorder exceed one or
more observed limits; fifteen-block p95 remains exploratory. Two-item prepend
stops with one measured pair when B0 accumulates only 19.790 ms in 9,984 actions.
The ordinary empty-batch upper-bound p50 is 0.258 µs but p95/max 5.865 µs exceeds
2 µs, so its bound cannot establish compliance. Empty-bulk B1 upper-bound
p50/p95 is 0.041/0.049 µs in Node and 0.028/0.065 µs in Chromium; only these
explicit conservative bounds justify passing cap-limited empty cases.

Setup entries below are **µs per construction/setup**, nearest-rank statistics
over 100 independently adequate measured blocks/variant, after five warmups.
p95 refers to block averages, never individual setup latency. Results remain
separate by fresh process; variants alternate first within each round.

| Node replica/cell      | B0 p50/p95/max/IQR                   | B1 p50/p95/max/IQR                  | Result                         |
| ---------------------- | ------------------------------------ | ----------------------------------- | ------------------------------ |
| 1, constructor N=10    | 20.375/160.267/994.626/50.262        | 24.607/223.992/589.409/38.979       | Median and p95 limits exceeded |
| 1, constructor N=1,000 | 1122.013/5984.878/26973.789/1947.703 | 1053.637/4751.158/8944.346/1211.374 | Pass this replica              |
| 1, subscribed N=100    | 318.731/2220.129/30171.617/463.193   | 367.164/2395.382/3225.427/576.381   | Median limit exceeded          |
| 2, constructor N=10    | 7.390/26.813/53.996/4.266            | 7.941/20.828/29.155/5.561           | Median limit exceeded          |

N=10's p50 differences are +4.232 µs and +0.551 µs against allowances
1.019 µs and 0.500 µs. Two replicas exceed the gate, triggering the planned
early stop. Their within-round median differences are +1.721 and +0.121 µs;
large variation and high host load do not establish an intrinsic constructor
cause. The finding is insufficient acceptance evidence under the declared
decision rule. The remaining 14 setup cells, including all Chromium setup
replicas, are unmeasured. No pooling, fourth replica or tail-cause investigation
was added.

#### Application retention, runtime footprint and allocation

The approved application-ownership criterion was applied explicitly. Calibration
found 273,832 bytes dominated by live product roots, then 5,856 bytes after abort
with the collection held: a 267,976-byte decrease. The selected array's weak
identity cleared and its backing-store node disappeared. Offline heap inspection
explains the mismatch: the probe assumed four-byte slots for its 32,768-element
array, but this Node build's actual backing store is **262,160 bytes** (eight-byte
slots), not the intended 128 KiB. This is a calibration-fixture defect, not an
application leak or evidence against the planner. The once-only calibration
stop was honored; there was no corrected rerun or retroactive target change.

Consequently, full-replacement/rollback/teardown/churn graph acceptance and all
90 allocation profiles are **unmeasured**, including the Chromium memory cells.
Prior direct-prototype allocation evidence remains valid in its original scope
but cannot fill these required package-path confirmations. The original failed
whole-heap result remains preserved.

Code/runtime accounting stays separate from application storage. In the two
candidate-only calibration heaps, code self sizes are 2,012,256 → 2,075,528 bytes,
shape metadata 548,848 → 553,784, native 30,639 → 30,639, and flat strings
2,684,155 → 2,685,795. Raw forced-GC used-heap ranges are 5,070,912–5,089,952 and
4,876,576–4,884,768 bytes. These are absolute probe-state observations, not
attributable candidate-minus-control footprint: no equivalent control memory
case ran after calibration stopped. Neither code categories nor raw heap totals
are silently subtracted to pass application retention.

The final recommendation is rejection because required acceptance evidence is
failed or unresolved, despite demonstrated bulk benefits and passing behavioral
coverage. This is not a claim that an intrinsic setup regression or retained
application defect has been established. Leave the reviewed implementation in
place and unsupported for the owner's decision; stop this bounded lab here.

### Stage 6: matched constructor diagnostic completed (2026-10-26 UTC)

**Diagnostic completed in 95.401 seconds; no adoption decision.** The earlier
92.8% constructor difference does not repeat as a stable candidate penalty
under matched live chunks and valid 20 ms blocks. This supports the direct
method/module-planner design over the private reorder bridge. It does not
establish zero overhead, setup/tail non-regression or full acceptance.

All three Node processes and three Chromium realms completed every planned
setup cell. Phase A takes 53.560 s, B 31.933 s, C 3.098 s and D 4.198 s; initial
smoke/imports and final coordination/cleanup account for the other 2.612 s.
Every phase fits its own ceiling and the whole run fits 300 s. All thirty
allocation profiles, three diagnostic CPU traces and sixteen one-shot bulk
actions complete. Node C01 stops at its first unresolved warmup block: B0
accumulates 20.012 ms in 53,056 calls; B1 accumulates only 12.432 ms in 99,968
calls, the last complete 64-action chunk below the 100,000-call cap. It has no
primary timing result and was not extended. No phase timed out, no product
probe failed and no cleanup errors were reported. No automatic continuation.

#### Does construction nearly double under matched conditions?

Values are microseconds per collection, **p50 / p95 / IQR**, with forty
measured blocks per variant per row. The last column is the median of the
within-round normalized B1/B0 and D0/B0 ratios; it is different from the ratio
of separate medians, so both are visible. Replicas are never pooled.

| Runtime / replica | B0                     | Direct B1               | Unused-method D0       | Paired median B1/B0 / D0/B0 |
| ----------------- | ---------------------- | ----------------------- | ---------------------- | --------------------------- |
| node 1            | 7.075 / 11.940 / 3.353 | 5.610 / 12.770 / 3.409  | 5.426 / 12.512 / 4.122 | 0.973 / 0.935               |
| node 2            | 8.720 / 13.713 / 4.142 | 7.456 / 11.396 / 3.543  | 7.287 / 10.577 / 4.356 | 1.030 / 0.961               |
| node 3            | 9.041 / 16.992 / 2.963 | 6.894 / 18.880 / 3.530  | 6.441 / 11.327 / 1.859 | 0.875 / 0.731               |
| chromium 1        | 4.404 / 5.812 / 1.272  | 4.744 / 9.457 / 1.709   | 4.217 / 10.165 / 1.251 | 1.238 / 0.996               |
| chromium 2        | 7.879 / 38.350 / 9.788 | 5.766 / 30.627 / 10.667 | 5.679 / 26.594 / 5.156 | 0.833 / 0.910               |
| chromium 3        | 4.457 / 7.356 / 1.547  | 4.301 / 6.324 / 1.292   | 4.404 / 5.704 / 1.353  | 0.965 / 0.990               |

The paired direct medians span 0.833–1.238; no replica reproduces a median
near 1.928. D0 medians span 0.731–0.996. Both designs overlap the control's
substantial variation. This is evidence against a repeatable near-doubling
from merely allocating an unused method/property. It does not prove that
the extra method is free or that larger factory code has no effect.

Some unfavorable readings remain: Chromium replica 1's direct p95 is 9.457 µs
versus B0 5.812 µs, a numerical allowance exceedance; the other two Chromium
replicas do not repeat it. Forty samples provide limited block-average tail
evidence, not a confirmed tail regression. Node replica 2 also has extreme
maxima (B0 232.607 µs, B1 91.487 µs, D0 29.372 µs), retained in the summary.
They are not trimmed or pooled away.

Other setup cells have fifteen measured blocks per variant; p95 equals the
maximum and is **exploratory**. Again values are p50 / p95 / IQR.

| Runtime / replica and cell   | B0                           | Direct B1                    |
| ---------------------------- | ---------------------------- | ---------------------------- |
| node 1, constructor-1000     | 385.630 / 611.727 / 258.399  | 373.352 / 785.754 / 131.192  |
| node 1, subscribed-100       | 124.830 / 3511.938 / 33.307  | 128.266 / 174.920 / 36.522   |
| node 2, constructor-1000     | 316.235 / 1132.367 / 271.444 | 319.373 / 1040.523 / 290.242 |
| node 2, subscribed-100       | 140.917 / 1110.614 / 42.048  | 147.201 / 256.630 / 58.976   |
| node 3, constructor-1000     | 339.537 / 1024.779 / 221.992 | 352.303 / 637.764 / 137.904  |
| node 3, subscribed-100       | 137.038 / 323.260 / 38.121   | 139.073 / 203.521 / 24.265   |
| chromium 1, constructor-1000 | 277.986 / 345.333 / 29.884   | 282.500 / 374.028 / 91.648   |
| chromium 1, subscribed-100   | 138.618 / 194.471 / 16.431   | 146.280 / 164.180 / 23.504   |
| chromium 2, constructor-1000 | 906.324 / 2397.187 / 789.445 | 651.250 / 2425.313 / 753.543 |
| chromium 2, subscribed-100   | 335.938 / 959.861 / 338.756  | 532.625 / 1629.271 / 580.048 |
| chromium 3, constructor-1000 | 259.438 / 304.485 / 61.053   | 264.013 / 288.264 / 58.462   |
| chromium 3, subscribed-100   | 132.270 / 225.664 / 30.773   | 124.256 / 157.617 / 16.603   |

The conspicuous Chromium replica 2 subscribed median is 335.938 → 532.625 µs,
with IQRs 338.756 and 580.048 µs. Its paired median is 1.411, while the other
Chromium subscribed paired medians are 1.055 and 0.948. This unfavorable
setup observation remains a concern; it has no supported attribution and
does not recur consistently across replicas. It cannot be dismissed as
proven host noise, nor can its fifteen-block p95 independently establish a
tail regression. Full setup non-regression is not established by this
diagnostic. Every completed primary setup block actually reaches 20 ms;
no short block is silently included.

#### What allocations does the direct implementation add?

Source adds one per-collection method/function and returned property, with
no new constructor scratch, registry, item loop or initialization list. The
new module planner is not called and has no allocated sample frame in
constructor profiles. VM storage/elision and compiled code remain separate
from a source-level allocation inventory; the former revision's 32-byte
closure size is not relabeled as a measurement of this revision.

Five independent profiles per variant/cell, 512-byte sampling with collected
objects included, give these **estimated sampled bytes/call**. Means are
reported alongside the paired median and full paired range to avoid selecting
whichever statistic resembles a predicted small constant.

| Cell             | Mean B0 / B1          | Mean / median paired additional bytes | Paired range        |
| ---------------- | --------------------- | ------------------------------------- | ------------------- |
| constructor-10   | 4392.14 / 4438.59     | 46.45 / 101.78                        | -154.67 to 198.59   |
| constructor-1000 | 216839.40 / 214654.00 | -2185.40 / -1075.00                   | -7414.00 to 2023.00 |
| C02              | 1649.58 / 1780.29     | 130.71 / 125.88                       | 100.87 to 167.56    |

Constructor N=10 estimates roughly +46 mean bytes, but individual paired
estimates range from −155 to +199 bytes. At N=1,000 the larger common item
allocation overwhelms the small added cost; negative deltas fall within sampling
variation and do not establish that the added method saves memory. These profiles
**do not resolve the exact added constructor bytes or separate closure from
property/context storage**. There are roughly 1,947–2,065 allocation samples
per N=10 profile and 1,311–1,414 at N=1,000; enough to observe the common
allocation but not a precise tiny difference. The requested precision remains
a demonstrated sampling limitation, with no extra profiles or new thresholds.

Factory-attributed mean samples at N=10 are 3,937.94 versus 3,960.73 bytes;
native Map/set differences account for much of the remaining net variation
despite identical source operations. No new item-scaled structure or bulk
scratch is observed during construction. This does not give an exact census
of every allocated function or property backing store.

The complete C02 append action has a clearer extra allocation: all five
paired estimates are +100.87 to +167.56 bytes/action, median +125.88, within
the original +256-byte small-action allowance. It includes API input
conversion, the returned plan, scratch/order construction and normal
journal/delivery work. The planner's attributed mean allocation is 434.39
bytes/action, but that is a gross site cost, not the net overhead: regular
insert/input/array work is replaced. Do not call the whole +126 bytes the
size of the returned plan object or transfer it to construction.

#### What does D0 distinguish?

D0 does not show a repeatable large method/property penalty. Direct B1 versus
D0 separate-median differences at N=10 are +0.184/+0.169/+0.452 µs in Node and
+0.527/+0.086/−0.103 µs in Chromium. Those values vary with the replica and
are smaller than the respective IQRs. This supplies no stable signature that
the larger method body/module planner causes the former near-doubling.
It favors retaining straightforward lexical integration; it does not identify
a unique VM optimization or quantify a precise closure allocation cost.

The three separately instrumented Node constructor blocks perform
1,792/2,304/2,816 constructions (B0/B1/D0) and accumulate 22.072/21.235/21.819 ms
of constructor time. They cannot be compared as equal-work spans without
normalization. CPU samples include factory work, GC and Inspector start/stop
overhead; they do not isolate a compiler/shape mechanism. The GC observer
delivers no entries before its recorded flush, although CPU profiles contain
GC samples. Its event-duration result is **unresolved, not zero GC**.
This instrumentation limitation is recorded; there was no rerun or replacement
of its samples. No attributable setup regression is claimed from these traces.

#### Small operations and bounded bulk diagnostics

Fifteen-block small timings are exploratory, in microseconds p50 / p95 / IQR.
C02 and C07 measured values fit the original numerical allowances in both
runtimes; these few samples do not independently establish population tails.

| Runtime / cell | B0                    | Direct B1             | Status             |
| -------------- | --------------------- | --------------------- | ------------------ |
| node, C01      | unmeasured            | unmeasured            | unresolved-minimum |
| node, C02      | 1.451 / 2.164 / 0.188 | 1.811 / 2.073 / 0.209 | complete           |
| node, C07      | 0.756 / 0.837 / 0.069 | 0.733 / 0.922 / 0.058 | complete           |
| chromium, C01  | 0.408 / 0.418 / 0.007 | 0.244 / 0.248 / 0.006 | complete           |
| chromium, C02  | 1.307 / 2.210 / 0.091 | 1.536 / 2.180 / 0.102 | complete           |
| chromium, C07  | 0.738 / 0.822 / 0.034 | 0.755 / 0.981 / 0.036 | complete           |

One checked cold action per variant/workload/runtime, N=1,000, gives:

| Runtime / diagnostic     | B0 action ms | Direct action ms | Observed ratio |
| ------------------------ | ------------ | ---------------- | -------------- |
| node, append             | 28.971       | 3.693            | 7.85×          |
| node, scatter-remove     | 45.686       | 4.397            | 10.39×         |
| node, replace            | 62.760       | 7.632            | 8.22×          |
| node, mixed              | 12.557       | 4.914            | 2.56×          |
| chromium, append         | 17.210       | 1.950            | 8.83×          |
| chromium, scatter-remove | 38.930       | 4.345            | 8.96×          |
| chromium, replace        | 43.350       | 6.345            | 6.83×          |
| chromium, mixed          | 10.005       | 2.315            | 4.32×          |

Append/removal/replacement use C=N; mixed uses C=100 with item updates and full
reorder. These sixteen calls take no repeated large calibration or warmups.
Membership ratios are 6.83–10.39× and mixed 2.56–4.32× in these individual
observations. They preserve the expected direction without spending minutes
remeasuring copying. They are diagnostic one-pair ratios, not p50/p95,
confirmation, proof of 2×/65% acceptance or replacements for unfinished
N=10,000 cells. Preparation, oracle/counters, reference capture, checks and
abort remain outside action clocks; full input conversion/validation and
commit delivery stay inside.

The raw report's SHA-256 is
`391b3573fe8950a08e7d10c6c53a9c230dee883c66ba9a147bef8ad6871f781d`.
Measured source and all earlier raw results are preserved. The outcome is a
completed diagnostic with explicit precision limits, not an incomplete
acceptance attempt or permission to publish the API.

### Stage 6: direct integration partial comparison (2026-10-26 UTC)

The later [constructor audit](#stage-6-constructor-audit-and-complete-reduced-continuation-proposal-2026-10-26-utc)
identifies a calibration defect and unequal retained chunk sizes. The numbers
below remain the original observations; their allowance comparisons establish
neither valid gate passes nor an attributable construction-cost ratio.

**Inconclusive and stopped early; no adoption.** The direct candidate is a
lab-only integration, SHA-256
`11448bf7cb9391b3d7751478a7df4006c912aa4f8db5ed0d9844176e789fb0c6`,
built from regular control
`1cbd0b38c8a9274c00c5a8f2692319ce84886a659ff69eef5786510fd5b96907`.
The prior candidate `406e39c85a96…` is unchanged. Direct source is 43,553 bytes,
146 lines and 6,523 bytes more than the regular source, including types and
comments. This is source footprint, not retained heap or compiled-code size.

Node v24.12.0, Darwin 24.6.0 x64, Xeon W-2140B, 64 GiB; initial load averages
5.98/9.96/13.03. Concurrent compilation elsewhere in the shared workspace was
observed while investigating the run's cost; no other session was interrupted.
The coordinator stopped its owned worker and saved partial results after
121.260 seconds. The report records a SIGTERM worker exit; this is an intentional
budget-feasibility stop, not a product correctness exception.

Three warmups and fifteen measured blocks per variant completed for each
Node setup cell. Inputs are outside constructor clocks; fixture/subscription
inputs are inside the separately defined subscribed-setup clock. Values are
microseconds per collection; p95 is nearest-rank **p95 of block-average setup
times**, not individual construction latency.

| Cell                    | B0 p50 / p95 / IQR         | Direct p50 / p95 / IQR   | Original gate assessment                              |
| ----------------------- | -------------------------- | ------------------------ | ----------------------------------------------------- |
| Constructor, N=10       | 5.91 / 47.13 / 4.93        | 11.39 / 72.36 / 11.78    | p50/p95 exceed allowances; cause unestablished        |
| Constructor, N=1,000    | 399.66 / 1,756.21 / 349.61 | 294.00 / 953.02 / 253.32 | Observed values fit; substantial variation            |
| Subscribed setup, N=100 | 185.95 / 352.50 / 136.67   | 198.92 / 371.70 / 142.19 | p50 exceeds 5%; p95 fits; difference inside variation |

Independent calibrated counts are B0/B1 1,024/2,048 for N=10, 64/8 for
N=1,000, and 64/64 for subscribed setup. Every calibration accumulated at least
20 ms. Unequal block sizes change GC pressure and block-average tail smoothing;
they limit causal interpretation even though normalized work/boundaries are
equivalent. These unfavorable results remain reported. They do not prove the
extra method closure causes a regression or justify a token/reorder bridge.

Large append reached calibration and one warmup pair only: 9.202 s/28.676 ms
at calibration, 41.473 s/147.783 ms at warmup (B0/direct). The warmup control's
setup is 1.685 s, action 41.473 s, validation 84.90 ms and abort 13.50 ms;
the candidate's are 746.32 ms, 147.78 ms, 13.80 ms and 5.20 ms. At least
50.851 seconds of the run is these two known control action clocks alone;
completed setup blocks total 1.911 timed seconds. Remaining wall time includes
semantic/count probes, calibration, fixture generation/GC, oracle/reset work,
coordination and the interrupted next fixture; there is no complete phase
ledger to attribute that balance. No CPU/GC cause is established.

The append pairs are diagnostic, not primary samples or confirmation. Their
large apparent benefit cannot pass an adoption gate. No primary action timing,
Chromium timing or allocation profile completed; those cells remain unmeasured.
The proposed retention clarification is not applied to pass any result.

### Stage 6: full-replacement heap evidence and explicit shared operation (2026-10-26 UTC)

**Retained-object finding: the measured collection graphs are released. The
remaining whole-heap residue is predominantly compiled module/probe code.**
This is a narrow ownership diagnosis, not acceptance of the discarded API or
a retroactive pass of the 16 KiB gate.

All twenty-four heaps completed in **194.086 seconds**, within the fixed
five-minute ceiling, in Chromium 156.0.8078.4. Candidate bytes exactly match
`406e39c85a96d6b9b341fc62647c3c33e41324e60d00f8a0f52cc9dbd0d0b116`
from public revision `dfc342e66eccb33a213fa778541ec5ca304c051f`; the actual
implementation was loaded read-only from its preserved source without restoring
the experimental export. The equivalent regular control includes the delivered
cleanup fixes, hash `1cbd0b38c8a9274c00c5a8f2692319ce84886a659ff69eef5786510fd5b96907`.
Commit and rollback each have one independently warmed realm per variant,
ten exact warmup lifetimes and one measured lifetime, six heap states and five
forced-GC used-heap observations per state. This is not a replicated timing or
allocation comparison. All completed value/order/identity checks held and no
cleanup errors were reported. Raw heaps and earlier results remain unchanged.

#### Measured objects and owners

Every initialized fixture has 10,000 incarnation objects, 10,000 item records,
10,002 subscriptions and the controller/signal pair. It has twelve public-method
closures in the control and thirteen in the candidate. The candidate's extra
`updateMembership` closure has **32 bytes shallow self size**; public-method
closure totals are 368 versus 400 bytes. Both returned collection objects have
12-byte shallow self size in these snapshots; their property backing storage
and shared shapes are separate, so these figures are not total instance sizes.
This confirms a per-instance closure cost without explaining setup p95 timing.

After a committed full replacement, all 10,000 old incarnations are gone. The
new 10,000 incarnations/records remain under the live collection. Only one old
record remains, retained intentionally by the probe's caller snapshot, with
the strong path `Window → caller fixture → oldRecord`. The old keys array is
also an explicit caller hold. The two membership/aggregate subscriptions remain;
all old item subscriptions have ended. Rollback instead preserves the original
incarnations, records, keys and subscriptions, as required.

After abort and releasing explicit snapshots, both variants have zero item/
membership/aggregate subscription objects and zero fixture controller/signal
objects. The live collection still owns its 10,000 current incarnations and
records: `Window → caller fixture → collection → size getter → context →
items Map → table → incarnation → record`. This is expected caller ownership,
not failed disposal; abort only ends subscriptions.

After dropping the collection and every caller reference, **zero measured
incarnations, records, subscriptions, signals or per-instance method closures
survive**, in either variant or transaction. Collection, owner, old/current
keys and old/current record identities are absent. The further fixed five-GC
observation gives the same result. Dataset-wide shape/value classification and
initialized/replacement node-ID cohorts agree. No per-collection map, journal,
callback or planner owner was found retaining these objects after drop.

The saved-graph parser initially counted allocation-site boilerplates as
instances and missed record labels stored as rope strings. Corrected analysis
decodes those strings and separates actual dataset values from shared templates
using field values and baseline identities. Earlier audits are preserved; no
capture, fixture or unfavorable byte reading was discarded or rerun.

#### What remains after release

| Variant/transaction | Used-heap residue (bytes) | Snapshot code delta (bytes) | Other snapshot deltas (bytes)        |
| ------------------- | ------------------------- | --------------------------- | ------------------------------------ |
| Control commit      | 16,024                    | 15,976                      | Native +48                           |
| Candidate commit    | 36,088                    | 35,792                      | Shapes +144, object +20, native −100 |
| Candidate rollback  | 35,956                    | 35,660                      | Shapes +144, object +20, native +148 |
| Control rollback    | 16,644                    | 16,576                      | Native +316                          |

Both post-drop snapshots give the same used-heap residue in each realm.
Snapshot self-size accounting and `Runtime.getHeapUsage` are different measures;
the columns are not an exact additive decomposition of used heap.

The actual surviving storage/root paths include:

- `Window → heap probe module → source-module binding → replace function →
  Code → InstructionStream`, and in candidate realms a source-module binding
  to `createCollection → Code → InstructionStream`. Added instruction streams,
  trusted byte arrays and deoptimization/feedback arrays dominate the residue.
- `replace → feedback cell/vector → nested closure feedback → AllocationSite
  → transition_info Object`. The candidate's 20-byte object is a V8 allocation
  template, not a dataset record. Its shape/dependent-code metadata is rooted
  through runtime feedback. Shared iterator-result maps/transition arrays also
  have native-context roots.
- `Global handles → DevTools UtilityScript → builtins.performance → browser
  timeline backing → PerformanceLongTaskTiming → TaskAttributionTiming` in
  rollback realms. This is browser/probe timeline storage, not a subscription
  signal or collection owner.

Thus the current capture demonstrates a **measurement limitation**: nonzero
whole-heap residue can persist after every measured collection-owned object
is gone, including a control exceeding 16 KiB. Compiled-code growth also belongs
to the framework/module's footprint and must remain reported; it is not free.
The historical ±25 KiB samples have no matching snapshots, so these heaps do
not identify their exact objects retrospectively. They establish the mechanism
and ownership distinction for the actual candidate under full replacement.
No threshold is weakened and the rejected version stays rejected.

The raw capture report hash is
`263c0c1efc5cf19574844908cd23b21c6a1dd2313c5541f5981b2040c16237fd`.
Full node identities and strong-root paths remain in local audit artifacts;
only compact counts, ranges and category summaries accompany this record.

### Stage 6: final accept-or-discard decision (2026-10-26 UTC)

**Decision: reject bulk membership for this version.** The owner's final rule
requires rejection when a required gate fails or remains inconclusive within
this bounded stage. The experimental runtime, public experimental export,
experimental-only tests and consumer fixture are discarded in public commit
`b623ff76f7378a458731b845ae736dfabe0e718d`. The regular API and independent
collection/store cleanup fixes remain. No permanent experimental branch or
second runtime implementation is part of this version.

#### Measurement correction and retained heap

The final harness uses one fixed CDP dispatcher and function declaration,
explicit remote-object-group release, ten exact warmup lifetimes covering
commit and rollback, and the same dispatcher/GC/control paths before baselines.
API inputs, oracles and reference captures are dropped before live/drop
observations. Each product lifetime is paired with a no-product probe lifetime;
all five GC readings remain raw evidence. This separates probe footprint from
product observations without subtracting a failed product gate into a pass.

All eight completed 128 KiB positive controls measure **131,096 bytes held and
zero bytes after release**. Across all sixty no-product comparisons, post-drop
deltas range **−1,932 to 84 bytes**; these fit the original 16 KiB budget.
The combined dispatcher/warmup correction therefore removes the earlier large
empty-probe/control residue in the measured cases. The separate contributions
of dispatcher compilation, warmup and remote handles were not isolated.

Sixty product lifetimes and sixty matched no-product lifetimes completed:
five samples per variant, case (C02/B06/Q02) and transaction (commit/rollback).
All six selected live-overhead cohorts fit their observed 10%/16 KiB envelopes.
The separate post-drop gate does not pass:

| Case/transaction | B0 post-drop observed range (bytes) | B1 range (bytes) | Outcome                                               |
| ---------------- | ----------------------------------- | ---------------- | ----------------------------------------------------- |
| C02 commit       | 0–7,768                             | 0–10,568         | Observed ranges fit                                   |
| C02 rollback     | 0–60                                | 0–60             | Observed ranges fit                                   |
| Q02 commit       | 0–352                               | 0–420            | Observed ranges fit                                   |
| Q02 rollback     | 0–60                                | 0–60             | Observed ranges fit                                   |
| B06 commit       | −24,504–24,836                      | −25,348–15,220   | Control numerical failure; candidate unresolved drift |
| B06 rollback     | −24,752–24,812                      | −25,384–25,496   | Numerical failures; attribution unresolved            |

Positive and negative full-replacement shifts survive the stable controls.
These are not established collection leaks; compiled/runtime footprint is
still a possible contributor. The fixed matched before/after heap-attribution
group was not reached before the deadline and supplies no evidence. Neither
negative deltas nor stable controls erase these numerical failures.

Churn keeps every observation: B06/B0 completed **100 cycles**, ranging
−9,612 to **19,472 bytes** above its original warmed empty baseline, exceeding
16 KiB at its maximum. It ended at −3,992 bytes, with −18,232 bytes from first
cycle to final; this is not evidence of monotonic product growth or a passing
instrument. B06/B1 completed **12/100 cycles** (−30,320 to 540 bytes) before
the deadline. Q02 churn in both variants was not started. The older uncorrected
100-cycle results remain preserved and do not substitute for corrected
acceptance. Churn acceptance is incomplete.

#### Small append allocations

Five independent action profiles per variant each contain 1,000 warmed,
restored C02 actions, with 512-byte sampling including collected objects.
Mean B0/B1 is **1,177.477 / 1,241.031 bytes/action**, or **63.554 extra bytes**.
The conservative observed gap envelope is **−6.300 to 124.004 bytes**, fitting
the unchanged 256-byte allowance. B0 p50/p95=max is 1,161.452/1,229.140;
B1 is 1,245.956/1,255.952. Actual-profile sample counts are 10,846/11,528.
Matched empty-loop profile means are 21.592/26.210 bytes and are not subtracted.
Twenty allocation profiles completed, including all ten matched controls.
The prior 375.871-byte failure and intermediate correction remain intact.

#### Setup and teardown

Every completed cell has five warmup and fifteen measured paired blocks of
100 independent lifetimes per variant. Values are microseconds. **p95 is p95
of block-average setup times, not individual setup latency**; with fifteen
measured blocks it equals the maximum. No outlier or losing replicate is
removed, and engines/sets remain separate.

| Set     | Engine/case  | Setup p50 B0 / B1 | Setup p95=max B0 / B1 | Setup outcome              |
| ------- | ------------ | ----------------- | --------------------- | -------------------------- |
| initial | Node Q02     | 90.589 / 102.520  | 179.491 / 256.083     | Numerical setup gate fails |
| initial | Node Q03     | 88.908 / 92.804   | 123.916 / 205.647     | Numerical setup gate fails |
| initial | Node C02     | 13.820 / 13.300   | 14.828 / 14.494       | Setup ranges fit           |
| initial | Chromium Q02 | 128.900 / 128.300 | 162.100 / 143.700     | Setup ranges fit           |
| initial | Chromium Q03 | 126.950 / 127.700 | 142.300 / 141.750     | Setup ranges fit           |
| initial | Chromium C02 | 14.450 / 14.400   | 21.050 / 22.600       | Setup ranges fit           |
| confirm | Chromium Q02 | 129.300 / 126.150 | 166.400 / 135.150     | Setup ranges fit           |
| confirm | Chromium Q03 | 124.050 / 122.450 | 143.300 / 131.750     | Setup ranges fit           |
| confirm | Chromium C02 | 15.200 / 14.650   | 21.500 / 20.850       | Setup ranges fit           |
| confirm | Node Q02     | 90.081 / 89.023   | 142.800 / 208.153     | Numerical setup gate fails |
| confirm | Node Q03     | 88.505 / 88.709   | 209.797 / 106.584     | Setup ranges fit           |
| confirm | Node C02     | 13.467 / 14.006   | 14.315 / 14.347       | Setup ranges fit           |

Node Q02 setup p95 fails in both sets. Initial increase is 76.592 µs against
17.949 allowed; the independent set increases 65.352 µs against 14.280 allowed.
The initial failure exceeds both IQRs; the independent excess does not exceed
the candidate's 51.649 µs IQR. This reproduces a numerical gate failure while
leaving its intrinsic cause uncertain. Node Q03's initial failure does not
repeat; Chromium setup fits in both sets. Passing cells do not waive Q02.

Other numerical findings remain in the compact summary: initial Node B04
teardown p95 121.770/154.961 µs against 12.177 allowed extra; initial Node B09
setup p95 1,267,483.040/1,431,515.411 µs against 126,748.304 allowed extra;
Chromium B04 teardown p50 75.000/80.000 µs against 3.750 allowed extra; and
independent Node C02 teardown p95 12.862/16.338 µs against 2 allowed extra.
Some are within the recorded IQR uncertainty. None is silently classified
as passing, nor attributed to a specific mechanism without evidence.

#### Large-operation benefit and independent repetition

Completed rows below have fifteen measured blocks after five warmups. Times
are **milliseconds per action**, normalized by each variant's own frozen
count. B02/B04/B06 are full append/removal/replacement at N=C=10,000; B09 is
mixed at N=10,000/C=1,000. Setup, reference capture, verification and abort are
outside total-action timing. All observed membership action gains exceed both
IQRs and the 2× target; completed mixed actions fit their non-regression gates.
These benefits do not override setup, teardown, retention or missing evidence.

| Set     | Engine/case  | Action p50 B0 / B1 (ms) | Action p95=max B0 / B1 (ms) | Median ratio |
| ------- | ------------ | ----------------------- | --------------------------- | ------------ |
| initial | Node B02     | 3727.400 / 18.529       | 5435.998 / 34.268           | 201.16×      |
| initial | Node B04     | 4129.470 / 323.332      | 4414.142 / 787.731          | 12.77×       |
| initial | Node B06     | 4727.500 / 355.218      | 6475.055 / 570.030          | 13.31×       |
| initial | Node B09     | 967.197 / 76.034        | 1877.715 / 146.703          | 12.72×       |
| confirm | Chromium B02 | 1375.965 / 6.058        | 2120.570 / 11.990           | 227.15×      |
| confirm | Chromium B04 | 3300.520 / 352.140      | 3578.705 / 380.545          | 9.37×        |
| confirm | Chromium B06 | 3677.875 / 353.235      | 3771.225 / 431.290          | 10.41×       |
| confirm | Chromium B09 | 715.445 / 65.375        | 846.245 / 73.780            | 10.94×       |
| confirm | Node B02     | 3489.380 / 15.611       | 3691.177 / 19.929           | 223.53×      |

Node append is independently repeated through the same package entry, with
the same frozen counts. Other large Node independent comparisons are incomplete:
B04 was interrupted; B06/B09 were not reached. Chromium's first timing group
ended during B02 calibration, so every completed large Chromium row in the
second group is a **first complete final set**, not independent confirmation.
Available calibration counts do not constitute a completed initial set. The
raw coordinator reports retain their original classification; the final
assessment corrects that distinction without changing samples. The missing
paired evidence prevents acceptance.

#### Budget, preservation and disposition

Initial, memory and independent timing groups consumed **480.047, 720.067 and
480.026 seconds**, including closure/bookkeeping at their unchanged eight-,
twelve- and eight-minute deadlines. No phase deadline was reset or transferred.
All partial rows and error reports remain. Owned workers and contexts closed,
with no reported cleanup errors. Deadline interruption is incomplete evidence,
not a product correctness failure or an accepted latency.

The measured candidate remains reproducible at
`dfc342e66eccb33a213fa778541ec5ca304c051f`, source SHA-256
`406e39c85a96d6b9b341fc62647c3c33e41324e60d00f8a0f52cc9dbd0d0b116`.
The frozen measured B0, original prototype, earlier package checkpoints,
corrected candidate and every prior report remain preserved. The pre-removal
manifest hash is `fb72313f5a16fa52cac9ca79f8e015b26aa96ef221a9392a3772d4492b07e868`.

Final raw-report SHA-256:
initial `ef9149932e83a957804bf25595a8a03a2516246ad37a701cd8f9b43d9d88d666`;
memory `a9def86485c6b5c2e8807363c859e9d96ec5a872913e95a98ab792a73b367940`;
independent group `4a5a9ac770fb5e91972e34d682ba13903692e1c5014792140b2dd582cdacc8d9`.

The regular collection/store sources retain hashes
`1cbd0b38c8a9274c00c5a8f2692319ce84886a659ff69eef5786510fd5b96907` and
`368915a3f34abd4cfc2ed4f709473f4529019dbf53e99537a09478a1024ddd5e`.
No Workbench changes or new regular API are part of this stage.

### Stage 6: single-append correction and retention calibration (2026-10-26 UTC)

**Outcome: attributable append overhead corrected; retention measurement is
not ready for acceptance.** These are bounded diagnostic reproductions, not
confirmation or a substitute for the unresolved setup and retention gates.

The original measured package checkpoint is preserved at public commit
`a92011d64fe6c979c9b6450b101d15daec8ac856`, source SHA-256
`7610e112f6a165f95bbcd13903c92d50e30853729499aca5a884926388405af1`.
The correction is commit `dfc342e66eccb33a213fa778541ec5ca304c051f`, source
`406e39c85a96d6b9b341fc62647c3c33e41324e60d00f8a0f52cc9dbd0d0b116`.
The intermediate conditional-set source, hash
`6aaeeed3671aba587fb20e0b0ad530a6ac219040e816e24d9b35bff49c208bf8`,
is separately preserved locally with its report. Frozen measured B0 and all
earlier reports remain intact. The independent regular cleanup commit is
`193c1620599e730bdecce22edee12839ea47a9df`; regression/consumer coverage is
`f1fa16a038dd782e21656cc34610ae683cde3f3c`; preceding findings are saved in
`154c3b4e1f16634c858ee7c419363d6ac93bf308`. Pending regular-API changes and
unrelated work were excluded from these commits.

#### Single append

Chromium 156.0.8078.4 used the actual experimental export and unchanged frozen
B0, with five 1,000-action profiles per variant per reproduction, alternating
first variant. Preparation/warmup/reset, validation and abort stayed outside
512-byte sampling; collected allocations were included. Every bounded action
chunk was below the 100,000-live-item cap. All executed result/identity checks
completed; no page or cleanup errors occurred. Runs took 3.108, 2.370 and
2.144 seconds respectively. Source bytes, profiles and reports are preserved.
Per-reproduction source hashes identify uncommitted measured bytes alongside
the recorded repository HEAD; the final commit preserves those same bytes.

Values below are estimated bytes/action, with n=5 independent profiles per
variant. At five profiles p95 equals maximum. The gap envelope is candidate
minimum minus control maximum through candidate maximum minus control minimum;
it is descriptive, not a confidence interval.

| Revision                         | B0 mean; p50 / p95               | B1 mean; p50 / p95               | Mean extra | Gap envelope    | 256-byte allowance     |
| -------------------------------- | -------------------------------- | -------------------------------- | ---------- | --------------- | ---------------------- |
| Measured checkpoint              | 1,193.422; 1,189.360 / 1,217.452 | 1,615.622; 1,621.904 / 1,647.620 | 422.200    | 353.872–469.552 | Fails diagnostic range |
| Conditional sets                 | 1,201.103; 1,192.984 / 1,226.964 | 1,459.050; 1,466.840 / 1,481.720 | 257.947    | 186.408–300.920 | Inconclusive           |
| Single insertion without buckets | 1,209.004; 1,207.352 / 1,222.848 | 1,276.607; 1,272.348 / 1,297.536 | 67.603     | 32.448–96.084   | Fits diagnostic range  |

Total allocation samples B0/B1 are 10,999/14,997, 11,074/13,548 and
11,139/11,876. The original 375.871-byte failure remains its original evidence;
the new before run reproduces the same direction with a larger control gap.
No original sample was replaced or pooled with these diagnoses.

The stacks and source identify unnecessary general-plan bookkeeping: a
removal set without removals, an insertion-duplicate set with only one
insertion, and a map plus bucket for that insertion's one anchor. The first
correction removes unnecessary sets but does not establish the gate. The
second stores the one anchor directly and uses the already prepared
incarnation. Multiple insertions retain duplicate detection and ordered
buckets; removals retain membership lookup. Both follow the same validation,
write guard, order construction, undo log and delivery. No global caching,
retained index or change to ordinary operations was introduced. Sampled sites
are attribution evidence, not exact individual object sizes.

Report SHA-256, before/intermediate/final:
`f9f6b4c0f4b82f70686926a10c3401845181c8c927524b14748e47e001273d8c`,
`6a5e8a35c654aa0da9890ec882b6735277da2d299c0f00acd0d329fd0ca596b4`,
`209546518dbcc4343aa01cb978777dd841995a49c3671bd4788d54ec862d67b3`.

#### Separate retention calibration

The exploratory control-size run took 7.377 seconds; its corrected repetition
took 6.933 seconds. Including diagnosis/correction, both finished within the
original 16:20:40.201–16:23:40.201 UTC deadline. Both used the final experimental
source above and frozen B0. Eight fresh realms per run cover both variants,
two fixed shapes and two fixed warmup conditions; each has three independent
product lifetimes (commit, rollback, commit), not independent OS processes.
All five GC readings per state remain saved; the table gives each lifetime's
post-drop residual in bytes against its own preceding empty baseline.

| Warmup              | Shape                     | B0 residuals           | B1 residuals           |
| ------------------- | ------------------------- | ---------------------- | ---------------------- |
| Canonical only      | Empty, N=10               | 20,188; 1,132; 6,232   | 20,576; 1,132; 6,232   |
| Canonical only      | Full replacement, N=1,000 | 111,660; 9,524; 21,716 | 110,200; 8,196; 22,820 |
| Ten exact lifetimes | Empty, N=10               | 5,372; 188; 16         | 5,200; 188; 16         |
| Ten exact lifetimes | Full replacement, N=1,000 | −19,532; 7,196; −6,320 | 912; 7,112; −5,840     |

Canonical warmup reproducibly leaves a shared first-use footprint exceeding
16 KiB. Exact workload/probe warmup reduces it, supporting a warmup mismatch
in the earlier measurement. It does not settle the instrument: in exact-warmed
B0 replacement, the held 128 KiB control registers only 99,148 bytes and its
release ends 31,948 bytes below the starting baseline. Seven other realms
register 131,136–131,168 bytes held and 60–72 bytes after release. That one
approximately 32 KiB baseline shift exceeds the retention budget and cannot
be dismissed or pooled away. The earlier 64 KiB exploratory run shows a
similar shift (−32,240 bytes), so this concern survives the fixture correction.
No extra warmups, samples or calibration search followed.

The fixed first-failure snapshot is from B0's canonical empty action, after
its first product lifetime. It contains no named per-instance `watchItem`,
`watchKeys`, `commitBatch`, `rollback`, `subscribe`, `deactivate`, `release` or
`updateMembership` closures; exported factory helpers remain. This supports
release of those instance contexts at that point. The heap includes native,
code, shape and shared closure storage; without a matched before snapshot
its categories cannot attribute the 20 KiB delta. Snapshot collection may
perturb that realm's later two lifetimes; other realms do not capture a heap.
Neither this inspection nor negative deltas prove leak freedom.

The corrected report hash is
`44bd97c6acc573ef3aa835f26ba23cb44d2412081887b65d553ebe08fd0d85d0`;
the preserved exploratory report is
`0f59fdd2f8026ea691a60ba8285d37a17a33c08c5c2a1679bff7ab2aa6cdbe43`.
All owned browsers closed. No substantial retention run, setup rerun or
adoption confirmation was launched.

### Stage 6: independent allocation and retention outcome (2026-10-26 UTC)

**Outcome: large membership allocation targets fit; one small-call allocation
gate fails; retained-heap acceptance remains unestablished. Adoption stays
blocked.** The independent phase completed in **34m 33.588s**, including the
runner correction and pause, within the original combined sixty-minute
deadline. All twenty cases completed: 800 complete allocation profiles (400
actual and 400 matched controls), 400 retained samples, twenty caller-held-key
observations, commit/rollback weak-reference probes, and four named 100-cycle
churn sets. Confirmation was not run.

The continuation reused 410 valid complete profiles and collected 390 missing
profiles. Twelve completed fresh small-action profiles remain exploratory,
not warmed-condition acceptance; one interrupted profile remains incomplete.
The original report, pre-correction source bytes and every artifact remain
preserved. The experimental implementation did not change during this phase.

#### Allocations

Values are arithmetic mean **estimated allocated bytes per action** across
five independent profiles, not exact allocation totals. Action timing includes
API input conversion and delivery; setup/reset/validation/counters are outside
sampling. Complete-lifetime profiles include input generation, creation,
action and abort. Matched controls are separate; no favorable control was
subtracted from the values below. Large profiles contain one action; Q/C
profiles contain 1,000 in bounded chunks. Sampling interval stayed 512 bytes,
including minor-/major-collected objects.

| Case | Action mean B0 / B1 (bytes)   | B1 / B0  | Lifetime mean B0 / B1 (bytes)  |
| ---- | ----------------------------- | -------- | ------------------------------ |
| B01  | 15,444,961.6 / 430,909.6      | 2.79%    | 16,265,315.2 / 1,281,331.2     |
| B02  | 1,504,797,301.6 / 4,661,298.4 | 0.31%    | 1,511,730,284 / 11,519,525.6   |
| B03  | 7,050,569.6 / 272,720.8       | 3.868%   | 7,693,279.2 / 896,860.8        |
| B04  | 708,719,379.2 / 2,625,309.6   | 0.37%    | 715,432,289.6 / 9,339,866.4    |
| B05  | 12,450,267.2 / 715,676        | 5.748%   | 13,084,736.8 / 1,343,952.8     |
| B06  | 1,213,737,101.6 / 6,680,596.8 | 0.55%    | 1,220,584,213.6 / 13,356,872.8 |
| B07  | 7,033,413.6 / 248,760         | 3.537%   | 7,295,776 / 503,726.4          |
| B08  | 118,730,647.2 / 371,386.4     | 0.313%   | 125,370,185.6 / 7,094,300      |
| B09  | 214,857,043.2 / 1,497,123.2   | 0.697%   | 221,693,963.2 / 8,221,864.8    |
| B10  | 1,214,169,855.2 / 7,044,036   | 0.58%    | 1,220,927,187.2 / 13,762,152   |
| Q01  | 145,692.91 / 16,616.217       | 11.405%  | 373,879.397 / 244,856.866      |
| Q02  | 7,698.898 / 7,178.386         | 93.239%  | 79,170.665 / 79,735.398        |
| Q03  | 2,013.435 / 2,338.147         | 116.127% | 72,269.786 / 72,742.432        |
| C01  | 259.635 / 387.354             | 149.191% | 11,617.263 / 11,738.18         |
| C02  | 1,239.793 / 1,615.664         | 130.317% | 13,054.543 / 13,480.648        |
| C03  | 1,621.75 / 1,771.77           | 109.25%  | 13,422.852 / 13,769.928        |
| C04  | 263.859 / 266.924             | 101.162% | 11,631.919 / 11,635.298        |
| C05  | 427.766 / 400.057             | 93.522%  | 12,250.962 / 12,264.341        |
| C06  | 243.368 / 206.798             | 84.973%  | 12,126.44 / 12,110.896         |
| C07  | 153.209 / 110.628             | 72.207%  | 11,955.661 / 11,985.73         |

The companion compact summary records n, p50, p95=max, observed range, IQR
and sample counts for every cell/scope. Observed profile ranges diagnose
uncertainty; they are not formal confidence intervals. All eight selected
membership-only large cases fit the 35% action-allocation limit over their
observed ranges, with mean reductions **94.25–99.69%**. All ten large-case
lifetime comparisons fit the unchanged 10% non-regression allowance. Mixed
savings are reported without imposing the membership-only reduction gate.

For 10,000 items, estimated action allocations fall from 1,504.797 MB to
4.661 MB for append, 708.719 MB to 2.625 MB for removal, and 1,213.737 MB
to 6.681 MB for replacement (decimal MB). Their p50/p95=max B0/B1 are:
append 1,504,800,284/1,504,818,596 versus 4,666,504/4,680,720 bytes;
removal 708,705,088/708,806,148 versus 2,630,744/2,645,252 bytes;
replacement 1,213,611,892/1,214,137,452 versus 6,548,220/6,984,472 bytes.
Total sample counts across five action profiles are 116,026/15,967,
483,647/5,471 and 598,406/21,902 respectively.

**C02, one bulk append at ten items, fails the small allocation gate.** Mean
additional allocation is **375.871 bytes/action**, against 256 allowed.
The observed profile-range difference is 345.872–414.220 bytes, entirely above
the limit. Baseline p50/p95=max is 1,242.104/1,246.656 bytes; candidate
1,612.432/1,638.848. Candidate sampled sites include about 576 bytes/action
at `updateMembership`, 266 at `Set`, 96 at `Map` and 281 at outer-batch
commit bookkeeping. These site estimates identify plan/order bookkeeping
costs, not precise object sizes. The ordinary single-insert control is
separate; do not hide this failed bulk call behind a different cell.

Q03, one removal at 100 items, has mean additional allocation 324.712 bytes,
but its observed difference range, 218.612–394.188, crosses 256:
**inconclusive at the approved sampling scope**, not passing. C01 empty bulk
and C03 two prepends have mean increases 127.718 and 150.020 bytes; observed
ranges fit 256 (C03 upper difference 250.512). That narrow observed margin
is not independent confirmation. No extra profiles or candidate tuning were
added to obtain a favorable result.

#### Retention and release

Every case/variant has five collection lifetimes each for commit and rollback,
with empty, initialized, live-after-action and dropped snapshots. All five
forced-GC observations at every state are preserved. A fresh realm is warmed
per case/variant, then reused for five independent product lifetimes; these
are not five independent fresh processes. Inputs, oracles and reference
captures do not remain in retained snapshots; caller-held keys are separate.

Of forty case/transaction cohorts, live-overhead observed ranges fit in 34,
are inconclusive in three (B07 commit/rollback and Q01 rollback), and contain
numerical failures in three: B03 rollback (47,920 bytes candidate excess in
one replicate), B04 commit (24,976/27,096 bytes in two replicates), and B05
commit (24,964/37,244 bytes in two). Failed replicates are not pooled away.

The post-drop **16 KiB gate is not established**. Both variants exceed it
in at least one replicate of all twenty commit cohorts and two rollback
cohorts (B02/B09); the other eighteen rollback cohorts fit. Even empty-bulk
and existing empty-batch controls show first-sample residuals around 20 KiB
in both variants. Large first-sample residuals are roughly 100–132 KiB.
Later samples near their own empty baselines do not erase those failures.
Cold probe/runtime/code initialization has not been separated from product
retention. This is an attribution/calibration gap, not proof of a
candidate-specific object leak and not an acceptance pass.

All commit/rollback weak-reference observations were false for the targeted
callback captures, selected values, discarded records and old keys in both
variants. This supports release of those objects, not the retained-byte gate.
Explicit caller-held old arrays were observed through disposal then released;
their post-release heaps also include the shared residual footprint.

All four churn sets completed exactly one hundred cycles. Values are bytes
above the original empty baseline, retaining every observation:

| Case / variant | After cycle 1 | Final   | Cycle 1 to final increase | Largest cycle observation |
| -------------- | ------------- | ------- | ------------------------- | ------------------------- |
| B06 / B0       | 115,556       | 156,148 | 40,592                    | 1,734,328                 |
| B06 / B1       | 112,756       | 155,852 | 43,096                    | 1,711,292                 |
| Q02 / B0       | 38,072        | 75,828  | 37,756                    | 101,240                   |
| Q02 / B1       | 37,848        | 68,548  | 30,700                    | 130,604                   |

These sets do not meet the no-growth/16 KiB rule. Growth occurs in steps and
includes transient high observations: B06/B1 reached 1,711,292 bytes at cycle
75 and returned to 155,852 at cycle 100. Late Q02 values plateau, but its
initial-to-final growth still exceeds budget. Do not discard spikes, redefine
the empty baseline after observing growth, or claim leak-free acceptance.
A bounded calibration/retainer investigation is needed before repeating a
large retained-heap acceptance set.

The preserved partial report hash is
`bb6880b743891ecba5bff9a51ff194c0f9167ea738f9055857270e0575321321`;
the final report hash is
`4d1fa1781d1f6cc177a57e609127ed47a6b246b563580b8e8eba673ba53dc1b2`. Aggregation reads saved data only and keeps
input snapshots. It combines only completed profiles with valid conditions;
it changes no threshold and authorizes no further runtime measurement.

### Stage 6: setup-only profiles and independent regular-core cleanup (2026-10-26 UTC)

The regular collection contained the same redundant normal-abort listener
removal as the experimental checkpoint. Its abort callback now clears the
already-ended registration before releasing the subscription; committed
removal and failure with a live owner still explicitly detach. The store's
normal abort already avoids that redundant call. Its initial-callback catch
nevertheless removed the listener again when the callback aborted and then
threw; it now detaches only while the record is still active. Original errors,
ended callbacks, and live-signal failure cleanup remain unchanged. These are
independent fixes to accepted cores, with no bulk API adoption or dependency
on the experimental module. The measured frozen baseline remains byte-identical.

Current regular collection hash:
`1cbd0b38c8a9274c00c5a8f2692319ce84886a659ff69eef5786510fd5b96907`;
store hash `368915a3f34abd4cfc2ed4f709473f4529019dbf53e99537a09478a1024ddd5e`.
The new work-count/lifecycle regression cases belong with their respective
core tests; this record makes no new standalone core timing claim.

Setup-only profiling completed in **16.699 seconds**, within its three-minute
ceiling, using three fresh Node processes and the actual experimental package
entry against frozen B0. Each CPU/allocation profile contained 1,000 subscribed
100-item setups, with validation and abort after the profiler stopped. CPU and
allocation profiling were separate; no mixed action/reset ran in either window.
Each process alternated first variant between CPU and allocation modes. The
maximum prepared product state was 100,000 live items. These are diagnostic
profiles, not another timing admission set or an individual setup-latency test.

Source inspection finds identical initial record conversion, item-map/order
construction and watcher registration algorithms. The checkpoint adds one
per-collection bulk-method closure and a returned method property. Its bulk
validation/order-building body is not executed during setup. The corrected
abort callback runs after the profiled setup, not inside it.

| Fresh process | First CPU variant | CPU-profiled setup total B0 / B1 (ms) | Sampled allocation B0 / B1 (bytes/setup) |
| ------------- | ----------------- | ------------------------------------- | ---------------------------------------- |
| 1             | B0                | 389.209 / 209.809                     | 167,378.016 / 167,881.048                |
| 2             | B1                | 216.272 / 315.207                     | 167,835.944 / 167,463.808                |
| 3             | B0                | 305.539 / 189.466                     | 167,143.264 / 167,293.952                |

Sampled setup allocation differences are +0.30%, −0.22% and +0.09%, changing
sign and lying within observed sampling variation. The first CPU-profiled
variant is slower in every process. That identifies a strong profile-order/
history effect in these totals, not a candidate-specific recurring CPU penalty.
Both profiles show record construction, item watchers, native event-listener
registration and GC. GC accounts for roughly 20–28% of CPU samples in both
variants; no unique heavy candidate setup path was isolated. The largest
allocation sites are `watchItem` (about 65 MB per 1,000 setups), collection
creation (about 37 MB), shared selector/caller work and native listener setup,
with comparable distributions across variants.

The GC event observer delivered no entries before these profile records were
captured, despite the CPU profiles containing GC samples. Its empty event
arrays therefore cannot prove the absence of GC or support precise event/time
correlation. Keep that instrumentation limitation, the profile perturbation,
shared warmup history, and holding a batch of fixtures explicit. These profiles
do **not** identify the cause of the previous admission setup-p95 tail, and do
not clear or discard it. No speculative setup optimization was applied.

Source hashes and all raw CPU/allocation profiles are preserved. The saved
setup-profile report hash is
`2e66b71e619959e43ba7e97411a0a6652f753c4a6838adad4f8fd5b4b7d036f3`.
Memory evidence proceeds independently under the owner's explicit amendment;
adoption remains blocked and confirmation does not follow from these profiles.

### Stage 6: checkpoint cleanup correction and admission stop (2026-10-26 UTC)

**Outcome: attributable cleanup inefficiency fixed in the experimental
checkpoint; acceptance still fails the setup-p95 gate.** No shipping behavior
was changed. The initial reproduction took 234.597 seconds and corrected
reproduction 212.534 seconds, each within its per-revision eight-minute ceiling.
Admission stopped after 72.702 seconds, preserving two complete Node cases.
There was no runner correction or deadline reset, and no cleanup error.

#### Reproduced append abort, before correction

Times are milliseconds per abort over seven measured blocks per process;
p95 equals max. No GC/profiler or counters ran in the primary blocks.

| Process | Baseline abort p50 / p95=max / IQR | Initial checkpoint abort p50 / p95=max / IQR | p50/p95 gate |
| ------- | ---------------------------------- | -------------------------------------------- | ------------ |
| 1       | 278.164 / 299.541 / 18.707         | 627.171 / 664.591 / 356.667                  | Both fail    |
| 2       | 268.516 / 355.050 / 9.170          | 679.649 / 786.110 / 464.878                  | Both fail    |
| 3       | 266.461 / 287.542 / 25.301         | 621.485 / 653.025 / 350.589                  | Both fail    |

The regression repeats through the actual experimental export, rather than
being confined to the old lab-copy import. The separately instrumented abort
measured 269.337 ms in B0 and 668.870 ms in B1. Native
`removeEventListener` accounted for 218/234 (93.2%) and 564/577 (97.7%)
CPU samples respectively; no GC event was observed during either abort.
Ordinary/pre-GC diagnostic medians were B0 318.272/277.727 ms and
B1 668.081/704.711 ms, three samples each. Pre-abort GC therefore did not
remove the checkpoint excess; its own cost is excluded from abort timing.

Source and the work-count reproduction identify a concrete redundant step:
abort callbacks are registered with `{ once: true }`, so platform dispatch
has removed the current listener before calling it. `release` then calls
`removeEventListener` for that already-removed registration, repeating native
listener-list searches. This inefficiency is present in both cores. The
profiles establish its dominant cost, not every runtime/heap detail behind
the difference in scan costs between the two variants.

The experimental callback now clears its `signal` and `onAbort` references
before deactivation. Committed removal, failed initial delivery and other
cleanup paths still explicitly remove live listeners. Ownership, idempotence,
per-subscription abort behavior and delivery registration order are preserved;
no signal pooling or automatic strategy switching was introduced.

#### Same comparison after checkpoint-only correction

| Process | Baseline abort p50 / p95=max / IQR | Corrected checkpoint abort p50 / p95=max / IQR | p50/p95 gate |
| ------- | ---------------------------------- | ---------------------------------------------- | ------------ |
| 1       | 293.135 / 362.726 / 97.181         | 3.434 / 4.391 / 1.282                          | Both fit     |
| 2       | 263.105 / 293.156 / 14.973         | 4.261 / 7.508 / 3.758                          | Both fit     |
| 3       | 276.542 / 292.493 / 33.218         | 3.646 / 5.260 / 1.417                          | Both fit     |

All three independent processes fit the unchanged abort allowances. Corrected
checkpoint append-action medians remained 24.579/21.601/25.512 ms versus
baseline 4,337.914/4,221.333/3,784.845 ms. Those action figures are diagnostic,
not the full acceptance matrix. In the corrected abort profile B0 still
spent 216/227 samples in native listener removal; B1 had nine total samples,
none attributed to that function, with a 6.948 ms instrumented abort. The
short candidate profile cannot support detailed cost percentages. Ordinary/
pre-GC candidate abort medians were 4.835/4.871 ms. This controlled correction
and repeated comparison support removal of the redundant native searches as
the cause of the improvement. They do not establish allocation or retention.

#### Corrected checkpoint admission

The fifteen-block summaries below are **per-action/per-setup block averages**
in µs, not individual setup latency. P95 equals max at this sample count.

| Case                                         | Action p50 B0 / B1 | Action p95=max B0 / B1 | Setup p50 B0 / B1 | Setup p95=max B0 / B1 | Setup IQR B0 / B1 |
| -------------------------------------------- | ------------------ | ---------------------- | ----------------- | --------------------- | ----------------- |
| Q01: 1,000 items, ten removals, no watchers  | 913.095 / 55.545   | 1,438.442 / 79.130     | 492.241 / 404.071 | 2,604.046 / 1,382.973 | 620.897 / 299.127 |
| Q02: 100 items, one mixed change, subscribed | 22.238 / 18.577    | 29.786 / 28.805        | 101.743 / 102.330 | 157.339 / 236.902     | 17.308 / 31.166   |

Q01 had no timing-gate failures. Q02 action and setup median allowances fit,
but setup p95 increased **79.563 µs** against **15.734 µs** allowed, an excess
of 63.829 µs beyond the allowance and larger than either IQR. The candidate's
worst setup block was round 11; the baseline's was round 6. Each cold block
averaged 100 lifetimes. Q02 teardown p95 also exceeded its gate: 145.378 µs
versus 128.342 µs, an increase of 17.036 µs against 12.834 µs allowed. That
4.202 µs excess is below either teardown IQR (10.907/9.634 µs); report it
as unresolved rather than an independently clear regression.

The driver stopped on the setup gate under the agreed rule against spending
memory/confirmation budgets on a clearly failed admission set. Its exploratory
fifteen-block p95 is sensitive to a single block. Host contention and previous
inconsistent setup findings remain threats, not reasons to discard the failed
block or call the gate passing. No setup-specific attributable correction has
been established by the abort investigation.

Only two of twenty Node cases completed; the remaining eighteen and every
Chromium case are unmeasured in this admission set. Allocation, retained heap,
churn and independent confirmation were **not started**, so those requested
phases remain incomplete. The prepared memory probe is unexecuted and supplies
no evidence. No numerical gate or sampling scope was relaxed to progress.

Read-only aggregation verified the unchanged saved reports, hashes:
initial reproduction `06f4f78739b7156d55f13dd1d45eeb6d199658a13330546b1e69f51dab3c9000`,
corrected reproduction `8e5a2bba4efe7163fc9f933c3e5b4e66870955bea4ec5137fcc83f4c7a0f3201`,
admission `2efe7311cce9214c80151ee8a32e201ba4bc683d340c7bbd89e8d84e7994aa73`.

### Stage 6: setup-tail reproducibility and intended bulk benefit (2026-10-26 UTC)

**Outcome: setup-tail difference inconsistent/unresolved; substantial intended
bulk action benefit; prototype unadopted.** Both phases completed in
245.385 seconds: 35.362 seconds for readiness/setup/attribution and
210.023 seconds for bulk, within the separate three-/five-minute ceilings.
No extra work was accumulated for timing resolution. All selected blocks
completed, with no cleanup errors or child-process stderr.

#### Setup, three independent process results

Values are µs per setup, summarized from **200 block-average setup times**
per variant per process, each averaging 100 setups. P95 is the 190th ordered
block average, not individual setup latency. Maxima and IQRs remain visible;
no tail block was excluded.

| Process | Setup p50 B0 / B1 | Setup p95 B0 / B1 | Setup max B0 / B1   | Setup IQR B0 / B1 | p95 increase / allowance | Gate  |
| ------- | ----------------- | ----------------- | ------------------- | ----------------- | ------------------------ | ----- |
| 1       | 98.672 / 98.506   | 162.815 / 214.517 | 277.006 / 384.833   | 9.334 / 10.440    | 51.702 / 16.282          | Fails |
| 2       | 99.157 / 99.523   | 132.980 / 139.380 | 524.736 / 487.997   | 8.900 / 8.156     | 6.400 / 13.298           | Fits  |
| 3       | 95.959 / 96.807   | 119.819 / 121.073 | 204.558 / 1,422.233 | 10.593 / 8.821    | 1.254 / 11.982           | Fits  |

All three setup p50 allowances hold. Setup p95 fails in one process, so the
precommitted diagnostic label is **inconsistent/unresolved**, neither repeated
in at least two processes nor absent in all three. In particular, process 3's
large candidate maximum is retained even though its p95 fits the allowance.
The original reduced-run failure remains a failed observation.

The paired moving-block bootstrap (ten consecutive paired blocks, seed 1,
2,000 replicates) gives these 95% intervals for the candidate/control setup
p95 ratio: process 1, **0.650–1.896**; process 2, **0.627–1.391**;
process 3, **0.865–1.109**. Corresponding intervals for excess beyond the
unchanged p95 allowance are −99.462 to +91.897 µs, −102.389 to +35.425 µs,
and −31.347 to +0.979 µs. All span zero. They describe uncertainty, not a
waiver of process 1's numerical failure or a reason to add samples.

| Process | Construction p95 B0 / B1 | Subscription p95 B0 / B1 | Teardown p50 B0 / B1 | Teardown p95 B0 / B1 |
| ------- | ------------------------ | ------------------------ | -------------------- | -------------------- |
| 1       | 68.389 / 92.879          | 99.585 / 95.852          | 41.438 / 41.125      | 54.323 / 52.374      |
| 2       | 57.810 / 57.990          | 74.739 / 83.239          | 41.382 / 41.647      | 56.631 / 59.873      |
| 3       | 48.991 / 47.000          | 72.578 / 71.530          | 42.060 / 42.358      | 58.887 / 59.673      |

All setup-case teardown p50/p95 allowances hold in this set. The elevated
component changes across processes; component p95 values are not additive.
The construction component retains the original fixture boundary, including
input generation and owner/controller creation; it is not pure factory time.
There is no repeatable isolated construction or subscription penalty here.

#### Separate attribution evidence

The instrumented passes observed 42 GC events in each variant, totaling
169.452 ms in B0 and 200.236 ms in B1; the longest events were 20.514 and
34.388 ms. Approximate timestamp overlap with setup was 74.360/159.024 ms,
and with abort 5.412/0.482 ms. These are diagnostic time correlations, not
allocation totals or proof that a candidate change caused a collection.
CPU sampling shows GC, event-listener registration, collection construction,
abort cleanup and record conversion in both variants; there is no isolated
candidate-only setup hot path established by these samples.

GC pauses can overlap setup in both implementations, but the instrumented
passes have different heap histories and do not identify the cause of the
untraced original tail. Combined with inconsistent primary results and high
initial host load, attribution remains **unresolved**. Do not relabel a
setup gate failure as a proven GC or host artifact.

#### Intended large operations: diagnostic action measurements

N = C = 10,000, representative subscriptions, exact original workloads.
Times below are milliseconds per complete logical action, including API
input conversion and synchronous commit delivery. Each summary has seven
measured actions; p95 equals max and is exploratory. Setup, validation and
abort are outside these action times.

| Workload             | Action p50 B0 / B1  | Action p95=max B0 / B1 | Action IQR B0 / B1 | Median speedup |
| -------------------- | ------------------- | ---------------------- | ------------------ | -------------- |
| Append               | 4,571.507 / 16.424  | 5,919.054 / 91.850     | 1,203.747 / 23.307 | 278.34×        |
| Scattered removal    | 4,514.037 / 320.571 | 4,678.942 / 355.495    | 470.258 / 128.967  | 14.08×         |
| Same-key replacement | 4,532.286 / 342.083 | 4,981.975 / 368.337    | 573.421 / 80.251   | 13.25×         |

All three meet the diagnostic comparison to the existing timing target:
candidate median at most half control, with the gain exceeding both IQRs.
Source inspection supports the intended mechanism: current scalar operations
copy/freeze the keys order for each supplied change, while the candidate
constructs one order from removal lookup and insertion buckets. These timings
do not measure allocated bytes or satisfy the 65% allocation gate.

#### Separate setup and cleanup: unfavorable evidence retained

| Workload             | Setup p50 B0 / B1 | Setup p95 B0 / B1   | Abort p50 B0 / B1 | Abort p95 B0 / B1 |
| -------------------- | ----------------- | ------------------- | ----------------- | ----------------- |
| Append               | 736.403 / 667.681 | 1,249.408 / 683.758 | 239.424 / 627.374 | 277.812 / 664.556 |
| Scattered removal    | 720.841 / 706.729 | 755.305 / 768.200   | 0.130 / 0.081     | 0.139 / 0.277     |
| Same-key replacement | 697.194 / 664.162 | 1,377.097 / 683.606 | 0.120 / 0.083     | 0.175 / 0.206     |

**Append teardown is slower in the candidate.** Its median increases by
387.950 ms (+162.03%) against the unchanged 11.971 ms allowance. Teardown
IQRs are 23.774/328.675 ms. Its p95 increases by 386.744 ms against a
27.781 ms allowance. This unfavorable diagnostic cannot be hidden by the
action speedup or used as an acceptance pass. The teardown after complete
removal/replacement is cheap because old item watchers have already ended
during the action; append leaves all original watchers alive until abort.
That lifetime difference explains why append exposes cleanup work, not why
the candidate's abort is slower than control in that case.

Both implementations use the same release/deactivate/unlink path for abort;
the recorded traces cover only the 100-item setup case, not large append.
Heap/GC or runtime conditioning is a hypothesis, not an attributable cause.
No extra profile or run was added to investigate this new finding. Medians
from separate phases are not summed into a claimed full-lifetime median.

Read-only aggregation recomputed these summaries and verified the saved
report remained unchanged (hash
`20e1bc6503fffbfd409f053d139f498307d9d37836b9535ad2340eb5bad02c16`).
Original reports and numerical gates remain intact. Both phases are complete;
no adoption, public API, memory acceptance, independent confirmation or later
roadmap stage follows from these diagnostics.

### Stage 6: approved reduced comparison stopped at setup gate (2026-10-26 UTC)

**Outcome: reject the current prototype for adoption under this run's gate;
the cause of the setup tail remains unresolved.** The initial phase stopped
after 65.045 seconds on the Q03 setup-p95 failure, as the owner instructed.
The run completed three of twenty paired cases in Node. The other seventeen,
all Chromium cases, allocation/retention and independent confirmation are
unmeasured in this run, not passing. Earlier partial measurements remain
separate evidence with their original qualifications.

All values below are microseconds per action or per fresh setup/teardown,
summarized from fifteen measured **block means**. Nearest-rank p95 equals the
maximum at this sample count; it is not an individual-operation p95.

| Case                                             | Action p50 B0 / B1 | Action p95 B0 / B1 | Setup p50 B0 / B1 | Setup p95 B0 / B1 | Setup IQR B0 / B1 |
| ------------------------------------------------ | ------------------ | ------------------ | ----------------- | ----------------- | ----------------- |
| Q01: 1,000 items, ten removals, no subscriptions | 636.753 / 43.338   | 660.267 / 156.228  | 398.663 / 416.325 | 939.157 / 967.008 | 279.208 / 62.963  |
| Q02: 100 items, one mixed change, subscribed     | 19.285 / 14.859    | 20.178 / 15.703    | 90.119 / 91.004   | 221.432 / 94.727  | 4.706 / 2.999     |
| Q03: 100 items, one removal, subscribed          | 8.464 / 4.642      | 9.061 / 4.949      | 87.699 / 87.892   | 112.307 / 217.282 | 2.858 / 2.768     |

Q01's setup median increase was 17.662 µs, below its unchanged allowance of
19.933 µs. Q02's earlier action-p95 concern did not reproduce in this clean
set. Neither observation erases the original unfavorable samples or constitutes
independent confirmation.

Q03's setup-p95 increase was **104.975 µs (93.47%)** against an allowance of
**11.231 µs**, leaving a 93.744 µs excess, larger than either setup IQR.
The candidate's worst setup block was round 6; the control's was round 13.
Each was the mean of 100 complete setups. In the candidate's worst block,
collection construction averaged 88.838 µs and subscription setup 128.079 µs.
No block was excluded or attributed to GC without evidence.
Q03 teardown p95 also exceeded its limit: 54.213 versus 48.373 µs, a 5.841 µs
increase against a 4.837 µs allowance. Its 1.003 µs excess is smaller than
either teardown IQR (2.170/1.802 µs), so it is a separate unresolved gate,
not an additional clear regression claim. Q01/Q02 had no timing-gate failures.

#### Bounded diagnostics

Setup diagnostics took about 13.773 seconds, and p95 diagnostics 1.543 seconds;
both completed inside their investigation limits. Each setup regime used
five warmups and fifteen measured blocks of 100 constructions per variant.

| Setup preconditioning          | End-to-end setup p50 B0 / B1 | Setup p95 B0 / B1 | Prebuilt-input construction p50 B0 / B1 |
| ------------------------------ | ---------------------------- | ----------------- | --------------------------------------- |
| Matched canonical records      | 493.543 / 534.497            | 628.060 / 629.194 | 347.262 / 349.888                       |
| Matched empty/getter shapes    | 614.082 / 519.157            | 873.994 / 650.087 | 387.920 / 401.004                       |
| Old asymmetric semantic prefix | 527.164 / 599.600            | 606.156 / 818.231 | 295.579 / 395.257                       |

The setup difference changes across regimes and common input/controller costs
also vary. This does not establish a consistent constructor penalty or prove
that asymmetric conditioning caused the original setup result. These are
diagnostics, not substitutes for the primary setup gates.

For Q02's diagnostic, block-mean action p95 B0/B1 was 24.011/22.497 µs with
subscriptions and counters off; 21.691/18.101 with counters on;
21.761/17.737 without subscriptions/counters; and 22.694/21.557 in the
counted-mode, unsubscribed control. Each has fifteen measured blocks of 100
actions. The counter-off subscribed individual-operation p95 was
27.338/22.430 µs over 1,500 actions, reported only as a diagnostic.
The original candidate action-tail regression did not reproduce. Fixed
diagnostic mode order and changing warm/heap state prevent interpreting these
values as a causal counter-overhead estimate. No GC-attribution pass was run.

The saved timing/calibration ledger accounts for 17.065 seconds of action,
14.970 seconds of validation, 14.493 seconds of reset and 0.132 seconds of
loop/unclassified work. These do not include the separate cold lifetimes,
diagnostics, validation-worker calls, reporting or coordination. The remainder
of the 65.045-second phase is not represented as product action time. Original
reports were not overwritten; read-only aggregation verified the new report
hash `2ee9b77ebcbc51f59223d5f637584871b02308edfc38a6f9b6812608953abce6`
and recomputed all three action summaries from saved block data.
Owned worker realms were terminated; cleanup reported no errors. No budget
was transferred or reset, and no rejected candidate was sent to confirmation.

### Stage 6: preserved-run time audit (2026-10-26 UTC)

This is read-only post-processing of the saved reports and inspection of the
saved runner source, not a new measurement run. Original reports were left
unchanged; their checksums matched before and after the audit. Existing raw
samples, partial/failed statuses, source identities and compact summary numbers
are preserved. The reduced matrix was a proposal when this audit was written;
the later approval and reduced run are recorded separately above.

| Portion of the initial 45-minute window | Recorded duration | What is established                                                                                          |
| --------------------------------------- | ----------------- | ------------------------------------------------------------------------------------------------------------ |
| Expanded fresh-fixture calibration      | 26m 16.975s       | 139 completed paired cells; 34.112s inside action timers; 25m 42.863s unsegmented non-action work            |
| Correction interval                     | 5m 29.863s        | Time from stopping the first runner to starting its replacement; includes code/preparation, not product work |
| Corrected run                           | 13m 13.205s       | 12 completed cells, plus an interrupted cell                                                                 |
| Total                                   | 45m 0.043s        | Original deadline, including the correction; watchdog scheduling accounts for the final milliseconds         |

Within the corrected run, the non-overlapping recorded components are:

| Component                                   | Time     |
| ------------------------------------------- | -------- |
| Warm action timers, including warmup blocks | 160.064s |
| Completed cells' calibration action timers  | 5.023s   |
| Complete cold lifetimes                     | 161.494s |
| Remainder, not phase-instrumented           | 466.624s |

Cold lifetime already includes 40.699s of setup, 119.566s of action and 1.230s
of teardown; these must not be added again. The unsegmented remainders combine
preparation, reset, validation, preflight, reporting and incomplete work. The
old runner did not record enough timestamps to divide those costs reliably;
do not label the whole remainder subscription setup or GC.

The code and saved counts explain the avoidable multipliers:

- Fresh-fixture calibration rebuilt and released every subscription for every
  trial, rather than limiting calibration to a small representative set.
- Restoring large warmed fixtures used repeated ordinary remove/insert calls
  in both variants, so untimed reset itself repeated the quadratic keys work
  the candidate was intended to eliminate.
- A common repetition count was chosen from the faster variant. At 1,000-item
  full removal, 50 actions per block made a roughly 37 ms control repeat
  unnecessary work after its timing resolution was already adequate.
- The corrected run executed 5,633,840 hot-loop actions including warmups.
  Two equivalent empty-change aliases accounted for 3,884,640 of those calls
  (69% of calls, **not a measured percentage of elapsed time**).
- The saved counts imply 91,514,000 reference-map entries rebuilt outside
  action timing. Non-control actions also used repeated JSON serialization,
  including eager failure-message serialization even when checks passed.
  These are count-derived work estimates, not allocation-profiler results.
- Full semantic probes ran before each timing block in the same process;
  B1's additional getter/empty-record cases gave its construction helpers a
  different input-shape history. This is an identified conditioning asymmetry,
  not proof that it caused the setup difference; the bounded proposal retains
  the exact old condition and adds isolated, matched preconditioning regimes.

**Additional runner finding:** warmed fixtures requested counted subscriptions.
In subscribed cells, counter increments and membership-history pushes therefore
ran inside action timers, contrary to the planned uninstrumented boundary.
The two no-subscription cells installed no callbacks and did not execute this
instrumentation; their setup and noise findings cannot be attributed to it.
Retain all twelve summaries, favorable and unfavorable alike. Classify the
subscribed action numbers as **instrumented exploratory evidence**; the others
are unconfirmed initial observations. None establishes complete acceptance.
The p95 comparison needs a clean repeat plus a counter-on/off diagnostic, not
dismissal. Cold setup/lifetime samples used uncounted subscriptions, so this
finding does not dispose of the setup concern. No new measurement was taken
to infer these causes.

### Stage 6: amended calibration and bounded initial set (2026-10-26 UTC)

**Outcome: inconclusive for adoption.** The cap amendment worked. All 40
paired small calibration cells (80 variant cells) reached 20 ms in each engine:
Node used 990–30,000 actions per cell, Chromium 990–41,000, stopping at the
first completed chunk that reached the duration target. No cap or performance
gate was relaxed.

The unnecessary full fresh-fixture calibration extension completed 139/184
paired cells and accumulated 34.112 seconds of action time over 26 minutes
17 seconds of wall time before it was stopped as a runner correction. That is
a limitation of this driver procedure, not a candidate correctness failure.
Its data is retained. The corrected steady-state driver then used warmed,
restored fixtures, fresh correctness probes and separate cold measurements,
without resetting the initial deadline. It completed 12/184 paired timing
cells before the original 45-minute ceiling. The next cell, N = 100,
C = 1, scattered removal with subscriptions, was interrupted and is
unmeasured. Unstarted cells remain unmeasured too.

Each completed cell has five warmup blocks and fifteen measured blocks per
variant. The table reports quantiles of **per-action block means** in
microseconds, not individual-action or presented-latency percentiles. At
n = 15 the nearest-rank p95 is also the maximum and is exploratory. The
[compact summary](collection.json) contains n, p50, p95, maximum and IQR for
every completed action, collection setup, subscription setup, total setup,
teardown and complete-lifetime metric, recomputed from saved raw blocks.

The subsequent source audit found counted subscriptions in the subscribed
warmed action timings. Those values remain unchanged as instrumented
exploratory evidence. The two no-subscription action cells did not execute
counters, and cold setup/lifetime metrics were uncounted throughout. No
favorable or unfavorable number was removed because of this finding.

| N     | C     | Workload       | Subscribed | B0 p50 / p95 / IQR, µs                 | B1 p50 / p95 / IQR, µs           |
| ----- | ----- | -------------- | ---------- | -------------------------------------- | -------------------------------- |
| 1000  | 10    | scatter-remove | No         | 710.898 / 752.063 / 7.206              | 45.042 / 49.416 / 0.411          |
| 10    | 0     | replace        | Yes        | 0.361 / 0.373 / 0.007                  | 0.496 / 0.523 / 0.026            |
| 10    | 2     | prepend        | Yes        | 2.289 / 2.879 / 0.089                  | 2.528 / 2.651 / 0.073            |
| 100   | 100   | prepend        | Yes        | 115.551 / 127.909 / 9.453              | 57.700 / 65.667 / 4.283          |
| 10    | 0     | item-only      | Yes        | 0.700 / 0.709 / 0.006                  | 0.691 / 0.701 / 0.008            |
| 1000  | 1000  | scatter-remove | No         | 37292.338 / 124699.292 / 37304.266     | 411.146 / 3743.541 / 50.922      |
| 100   | 10    | scatter-insert | Yes        | 13.014 / 15.698 / 1.378                | 12.331 / 14.040 / 0.624          |
| 100   | 10    | replace        | Yes        | 92.138 / 96.032 / 2.802                | 21.080 / 26.330 / 1.112          |
| 100   | 100   | append         | Yes        | 88.024 / 112.114 / 15.828              | 61.439 / 66.421 / 5.342          |
| 10000 | 10000 | mixed          | Yes        | 4345645.815 / 6485489.589 / 387024.963 | 28036.701 / 39288.241 / 1740.293 |
| 100   | 1     | mixed          | Yes        | 20.011 / 21.365 / 0.959                | 15.454 / 44.414 / 1.194          |
| 10    | 0     | scatter-insert | Yes        | 0.366 / 0.385 / 0.011                  | 0.528 / 0.712 / 0.039            |

The results show why a bulk path is worth investigating, but do not satisfy
the complete decision rule:

- Removing ten of 1,000 items without subscribers has a 710.898 → 45.042 µs
  initial median, with the difference larger than both IQRs.
- Removing all 1,000 items has a 37.292 → 0.411 ms median, about 90.7× by
  ratio. **The agreed noise criterion is not met in this set:** the control's
  37.304 ms IQR exceeds the 36.881 ms median difference. Do not turn the large
  ratio into a confirmed speedup claim.
- The 10,000-item full replacement and reverse with subscribers has a
  4,345.646 → 28.037 ms median, and its difference exceeds both IQRs. This
  C = N mixed cell has no surviving items to update; it measures membership
  replacement plus reorder, not a partial mixed action with survivor updates.
- Small completed action medians remain within their recorded allowances;
  for example the 10-item, two-entry prepend rises 2.289 → 2.528 µs.
  That does not establish unmeasured single-change or existing-operation cells.
- **Setup concern:** in the 1,000-item/ten-removal/no-subscriber cell, fresh
  setup rises 378.547 → 423.309 µs (+11.8%). Its 44.762 µs difference exceeds
  the 18.927 µs allowance and both setup IQRs (29.296/26.296 µs). This fails
  the initial setup limit; there is no confirmation set to establish scope.
- **p95 concern:** the subscribed 100-item, one-change mixed cell has an action
  block-mean p95 of 21.365 → 44.414 µs, beyond the 2.136 µs permitted increase.
  The median improves; it does not cancel this exploratory upper-percentile
  result. Independent confirmation is missing.

The 45-minute ceiling ended the set, not the 100,000-action cap. No extra
run was launched to replace unfavorable samples or recover omitted cells.
The timing matrix, independent confirmation, Chromium main timing, allocation
profiles, retained heap, churn, weak-reference release checks and shipping
confirmation remain incomplete or unrun. In particular, **the 65% allocation
gate and retained-memory gates have no measured result**. Source inspection
of the single-order construction is not allocation evidence.

The record retains the earlier smoke/calibration outcome and the stopped
runner attempt. No version of the prototype was tuned during these
measurements. No public API adoption, shipping implementation, package
verification suite, commit or stage 2 work follows from this partial result.

### Stage 6: smoke and early calibration stop (2026-10-26 UTC)

**Outcome: inconclusive performance; stop before main runs.** Each engine's
smoke exercised six workload groups at N = 100, 1,000 and 10,000, using
C = ceil(N/10), both without subscribers and with the specified item,
membership and membership-count subscriptions. Both variants matched the
independent order/value oracle, survivor identities and each other's delivery
counts in every smoke cell. These are single correctness samples per cell,
not latency samples or evidence that the performance gates hold.

| Workload                                    | Membership callbacks/action | Item reads and callbacks/action | Wide reads/action | Wide callbacks/action |
| ------------------------------------------- | --------------------------- | ------------------------------- | ----------------- | --------------------- |
| Append, prepend, scattered insertion        | 1                           | 0                               | 1                 | 1                     |
| Scattered removal                           | 1                           | 0                               | 1                 | 1                     |
| Same-key replacement                        | 1                           | 0                               | 1                 | 0                     |
| Mixed field updates, membership and reverse | 1                           | C                               | 1                 | 0                     |

Counts exclude initial registration, apply to the subscribed fixture and were
identical for B0/B1 in Node/Chromium. All delivery counts were zero in the
unsubscribed fixture. Replacement reported each replaced key in both metadata
lists even though the key strings and order were unchanged. Both variants'
per-cell results and histories remain local; no raw samples are committed.

Calibration began with the mandatory cheap control, B0's empty batch on a
10-item collection without subscribers. Recomputed from its raw chunk
durations:

| Calibration observation    | Value                                                              |
| -------------------------- | ------------------------------------------------------------------ |
| Measured actions           | 10,000, across ten chunks of 1,000 independent collections         |
| Accumulated action time    | 12.341604 ms                                                       |
| Approved minimum per block | 20 ms                                                              |
| Weighted action time       | 1.2341604 µs/action; calibration only                              |
| Clock-pair sample          | 10,000 pairs; p50 0.123 µs, p95 0.209 µs, max 417.65 µs            |
| Resolution decision        | Unresolved under the approved cap/minimum-duration procedure; stop |

This single calibration block cannot establish p50/p95 regression acceptance
or compare B1 with B0. The failure is in satisfying the planned measurement
boundary: it does not establish that the API is slow, fast, or incapable of
meeting the product thresholds. The owner requested stopping at this point;
there was no retry with relaxed rules.

**Source cost, not a speed result.** The unformatted candidate module is
41,132 bytes/1,099 lines against 36,779 bytes/994 lines for the control:
4,353 bytes/105 lines added. This includes the runtime method and import-path
adjustment, not final public typedefs, tests or guidance. Source inspection
shows one indexed original-order pass, traversal of insertion buckets, one
frozen next order and O(C) map/undo entries. This is algorithmic evidence,
not measured allocation evidence.

**Unmeasured:** B1 calibration, remaining Node calibration cells, Chromium
calibration, main timing and confirmation sets, standalone API timings,
allocation profiles, lifetime/retained-heap comparisons, forced-GC weak-reference
checks, churn, diagnostic D0 and shipping-path confirmation. The 2×/65%
membership-only gates, mixed-action regression gates and all memory gates are
unresolved. No unmeasured cell is treated as passing.

### Initial comparison (2026-10-25)

Medians across pages of each page's median, with the full-flow lab's
meaningful-difference rule.

| Measure                                  | Items          | Hand-written     | Lattice          | Difference         | Meaningful         |
| ---------------------------------------- | -------------- | ---------------- | ---------------- | ------------------ | ------------------ |
| Mount, µs per item                       | 10, 100, 1,000 | 17.0, 16.7, 20.4 | 19.2, 18.9, 23.1 | +2.2 to +2.7, +13% | Yes, except at 100 |
| Teardown, µs per item                    | 10, 100, 1,000 | 10.3, 10.1, 11.2 | 11.4, 12.0, 20.2 | +1.1, +1.9, +9.0   | Yes                |
| Single-item update, µs                   | 10, 100, 1,000 | 1.75, 1.94, 1.78 | 3.00, 16.5, 245  | +1.2, +14.6, +243  | Yes                |
| Reorder, µs per item                     | 10, 100, 1,000 | 2.28, 3.28, 12.0 | 2.53, 3.43, 12.1 | +11%, +4%, +1%     | Only at 10         |
| JavaScript heap per item, bytes          | 100, 1,000     | 575, 417         | 991, 926         | +417, +509         | Yes                |
| Bytes allocated per update, steady state | 10, 100, 1,000 | 77, 77, 77       | 276, 636, 4,249  | grows with items   | Yes                |

Counts per operation were identical in both variants for every DOM effect: an
update made two label setter calls, one assignment and its reflection echo,
one text write and one attribute write; a reorder of N items made N − 1 moves
and created or removed no row; teardown disconnected N observers. Lattice made
2N selection reads per update, two per row, and one per reorder; the control
made none.

### Causes

- **Updates.** Each row binding subscribes to its item's label and done
  selections, both declared on the shared `byId` field. Changing one item
  replaces `byId`, so the commit considers all 2N subscriptions and calls 2N
  `read`s to update one row. The update also copies the map of items, which
  accounts for the allocation growing with N. The cost is linear in the
  number of items; a field index would not help, since every row subscription
  declares the same field.
- **Teardown.** Aborting a row's lifetime deactivates its two subscriptions.
  Outside a delivery, each deactivation compacts the whole subscription array
  at once, so tearing down N rows one by one does work proportional to N².
  The cost per item is small at 10 and 100 items and grows to +9 µs per item
  at 1,000.
- **Mount and memory.** The binding setup the full-flow lab measured costs
  +2 to +3 µs and about 420 to 510 bytes per item here, including the two
  subscription records each row holds.

### Stage 2: bulk compaction (2026-10-25)

Variant 1, the current store, against variant 2, compaction in bulk, both
running the Lattice collection. Timing 00:58–01:03 UTC with load rising from 7
to 24; churn, cycles and counted 01:03–01:04 with load 15 to 22. The lab copy
passed all 48 of the store's unit tests before the runs.

| Measure                                                   | Items          | Variant 1        | Variant 2        | Difference     | Meaningful    |
| --------------------------------------------------------- | -------------- | ---------------- | ---------------- | -------------- | ------------- |
| Teardown, µs per item                                     | 10, 100, 1,000 | 11.5, 12.6, 20.7 | 11.7, 12.1, 13.5 | +2%, −5%, −35% | At 1,000 only |
| Mount and teardown of 100 items, ms per cycle             | 100            | 4.11 (IQR 0.18)  | 3.87 (IQR 0.16)  | −6%            | Yes           |
| Retained after 20 cycles, bytes                           | 100            | 16,300           | 16,296           | 0%             | No            |
| Removing 900 of 1,000 items one at a time, µs per removal | 1,000          | 155              | 152              | −2%            | No            |
| Single-item update after that churn, µs                   | 100 remaining  | 20.3             | 20.6             | +1%            | No            |
| Heap after that churn, bytes                              | 100 remaining  | 1,774,772        | 1,781,136        | +0.4%          | No            |
| Mount, update and reorder                                 | 10, 100, 1,000 |                  |                  | −2% to +10%    | No            |

Removing items one at a time stayed near 150 µs per removal in both: each
removal also filters the order and runs the reconciler over the remaining
rows, which compaction does not change. The heap after churn differs by the
inactive records allowed to wait. Both variants passed every counted check,
including that a removed row's callbacks never run when its item changes.

**Outcome.** Under the stage's rule, variant 2 is adopted: teardown of 1,000
items is 35% faster, cycles 6% faster, and nothing is meaningfully worse. It
is implemented in `src/modules/store/store.js` with a unit test that a
deactivated subscription's callbacks are released at once, while its record
still waits; the store's unit tests, the example's tests and the browser tests
pass.

### Stage 3: collection state shape (2026-10-25)

Variant 1, the shared map, against variant 3, one store per item. Medians
across pages of each page's median; "meaningful" uses the stage 1 rule.
Store-per-item p95 and maximum across pages stayed within 10% of its median
except mount at 100 items (35.2 against 21.0 µs) and teardown at 100 items
(23.4 against 15.4 µs), single pages in both cases.

| Measure                                       | Items          | Shared map         | Store per item      | Relative         | Meaningful       |
| --------------------------------------------- | -------------- | ------------------ | ------------------- | ---------------- | ---------------- |
| Label update, µs                              | 10, 100, 1,000 | 3.81, 21.2, 339    | 2.68, 2.99, 5.20    | −30%, −86%, −98% | Yes              |
| Done toggle, µs                               | 10, 100, 1,000 | 3.63, 22.0, 342    | 2.84, 4.73, 31.8    | −22%, −79%, −91% | Yes              |
| Bulk label update of 10 items, µs             | 10, 100, 1,000 | 19.4, 39.5, 361    | 19.8, 19.9, 21.6    | +2%, −50%, −94%  | At 100 and 1,000 |
| Bulk label update of every item, µs           | 10, 100, 1,000 | 19.2, 220, 3,665   | 19.4, 194, 3,820    | +1%, −12%, +4%   | At 100 only      |
| Insertion at the end, µs                      | 10, 100, 1,000 | 34.3, 92.6, 849    | 30.3, 54.5, 355     | −12%, −41%, −58% | At 100 and 1,000 |
| Removal, µs                                   | 10, 100, 1,000 | 31.6, 125, 2,469   | 35.0, 64.3, 400     | +11%, −49%, −84% | At 100 and 1,000 |
| Reorder, µs per item                          | 10, 100, 1,000 | 2.64, 3.79, 12.6   | 2.44, 3.72, 12.7    | −8% to +1%       | No               |
| Mount, µs per item                            | 10, 100, 1,000 | 20.0, 20.8, 23.2   | 22.3, 21.0, 25.7    | +12%, +1%, +11%  | At 10 and 1,000  |
| Teardown, µs per item                         | 10, 100, 1,000 | 11.4, 11.9, 13.0   | 15.1, 15.4, 17.0    | +33%, +30%, +31% | Yes              |
| JavaScript heap per item, bytes               | 10, 100, 1,000 | 2,618, 920, 807    | 3,206, 1,692, 1,486 | +22%, +84%, +84% | Yes              |
| Bytes allocated per label update              | 10, 100, 1,000 | 268, 652, 4,253    | 85, 85, 85          | −68% to −98%     | Yes              |
| Bytes allocated per done toggle               | 10, 100, 1,000 | 247, 632, 4,231    | 114, 138, 138       | −54% to −97%     | Yes              |
| Bytes allocated per bulk update of every item | 10, 100, 1,000 | 700, 4,753, 44,993 | 986, 9,039, 89,424  | +41%, +90%, +99% | Yes              |

Selection reads and open-count derivations per operation, from the counted
pages (N items):

| Operation                    | Shared map reads | Shared map derivations | Store per item reads | Store per item derivations |
| ---------------------------- | ---------------- | ---------------------- | -------------------- | -------------------------- |
| Mount                        | 2N + 2           | 1                      | 3N + 2               | 1                          |
| Label update                 | 2N + 1           | 1                      | 1                    | 0                          |
| Done toggle                  | 2N + 1           | 1                      | 2                    | 1                          |
| Bulk label update of k items | 2N + 1           | 1                      | k                    | 0                          |
| Insertion                    | 2N + 4           | 1                      | 5                    | 1                          |
| Removal                      | 2N + 2           | 1                      | 2                    | 1                          |
| Reorder                      | 2                | 1                      | 2                    | 1                          |

Every DOM effect count was identical between the variants at every step and
size: label and disabled assignments, text and attribute writes, moves, rows
created and removed, observers attached and disconnected.

**Reading the costs.**

- **Item updates** cost the same at every size with stores per item: one
  store, one or two subscriptions, about 85 bytes. The shared map pays 2N
  reads, a derivation and a copy of the map on every update.
- **The collection-wide derivation stays linear.** A done toggle at 1,000
  items costs 31.8 µs against 5.2 µs for a label update, because the open
  count is recomputed over every item's store. The shared map derives on every
  change to the map, including label changes that cannot alter the count;
  stores per item derive only when membership or a `done` changes. A bulk
  update of `done` on k items would derive k times with stores per item, one
  per commit; that workload was not in the plan and was not measured.
- **Bulk updates of every item** take about the same time in both, dominated
  by the row writes, and stores per item allocate about twice as much: one
  snapshot and delivery per item instead of one map copy. Subscribers of
  other items can observe the update partly applied between those commits.
- **Memory and teardown** are the variant's costs: about 680 to 770 bytes and
  4 µs more per item at 100 and 1,000 items. Each item adds its store and the
  summary binding's subscription and controller for its `done`; this lab did
  not attribute the bytes between them.

### Stage 3 follow-up: memory attribution and bulk done updates (2026-10-25)

**Bulk done updates**, medians across pages:

| Measure                                            | Items          | Shared map         | Store per item         | Ratio               | Meaningful |
| -------------------------------------------------- | -------------- | ------------------ | ---------------------- | ------------------- | ---------- |
| Bulk done update of 10 items, µs                   | 10, 100, 1,000 | 21.4, 40.6, 356    | 29.0, 43.1, 189        | 1.36×, 1.06×, 0.53× | Yes        |
| Bulk done update of every item, µs                 | 10, 100, 1,000 | 20.6, 240, 4,245   | 28.0, 443, 21,065      | 1.35×, 1.84×, 4.96× | Yes        |
| Bytes allocated per bulk done update of every item | 10, 100, 1,000 | 928, 7,874, 79,808 | 1,448, 16,285, 162,512 | 1.56×, 2.07×, 2.04× | Yes        |

Per call, a bulk done update of k items made the same row DOM effects in both
variants. The shared map read 2N selections, derived the open count once and
wrote the summary text once. Stores per item read 2k selections, derived the
open count k times, each over every item, and wrote the summary text k times:
every intermediate count was written to the DOM during the synchronous call.
That does not establish that the browser painted any of them, but each write is
unnecessary work and an intermediate state that subscribers and DOM observers
can see. At 1,000 items, updating every
item's `done` with stores per item did about 1,000 derivations over 1,000
items, 21 ms against 4.2 ms.

**Memory attribution**, JavaScript heap per item, medians across 10 pages:

| Items | 1. Shared map | 1n. Without summary | 3. Store per item | 3n. Without summary | Bare store |
| ----- | ------------- | ------------------- | ----------------- | ------------------- | ---------- |
| 100   | 922           | 920                 | 1,693             | 1,434               | 527        |
| 1,000 | 811           | 806                 | 1,486             | 1,335               | 521        |

At 10 items every variant varies by about 550 bytes between pages, too coarse
to attribute. At 100 and 1,000 items, of the 675 to 770 bytes per item store
per item adds:

- **the item store**, 3n minus 1n: 514 and 529 bytes, matching a bare store
  holding the same two fields, 527 and 521 bytes; a store's own size is nearly
  all of it, and the row bindings cost the same whichever store they watch;
- **the summary binding's per-item subscription and controller**, 3 minus 3n
  less the shared map's single summary subscription: 257 and 146 bytes.

Teardown time was not attributed; the extra subscription and controller per
item, and releasing a store per item, are the only per-item differences in
teardown.

### Stage 4: keyed collection prototype (2026-10-25)

Variant 4, the keyed collection, against variant 1, the shared map, with
variant 3, store per item, for reference. Medians across pages of each page's
median; "meaningful" uses the stage 1 rule against the shared map.

| Measure                                             | Items          | Shared map         | Store per item         | Keyed collection       | Keyed vs shared map | Meaningful       |
| --------------------------------------------------- | -------------- | ------------------ | ---------------------- | ---------------------- | ------------------- | ---------------- |
| Label update, µs                                    | 10, 100, 1,000 | 3.85, 21.4, 339    | 2.80, 3.08, 5.22       | 2.81, 3.17, 4.92       | −27%, −85%, −99%    | Yes              |
| Done toggle, µs                                     | 10, 100, 1,000 | 3.70, 22.1, 341    | 2.88, 4.79, 31.5       | 3.14, 6.09, 45.9       | −15%, −72%, −87%    | Yes              |
| Bulk label update of 10 items, µs                   | 10, 100, 1,000 | 20.1, 39.7, 358    | 20.3, 20.2, 21.8       | 21.3, 22.8, 22.8       | +6%, −42%, −94%     | At 100 and 1,000 |
| Bulk label update of every item, µs                 | 10, 100, 1,000 | 19.4, 222, 3,680   | 19.8, 198, 3,800       | 20.7, 220, 3,450       | +7%, −1%, −6%       | At 10 only       |
| Bulk done update of 10 items, µs                    | 10, 100, 1,000 | 20.3, 40.6, 356    | 27.5, 42.8, 257        | 22.2, 25.8, 64.3       | +9%, −36%, −82%     | Yes              |
| Bulk done update of every item, µs                  | 10, 100, 1,000 | 20.4, 237, 3,945   | 27.5, 442, 23,275      | 22.0, 233, 3,635       | +8%, −2%, −8%       | At 10 and 1,000  |
| Insertion, µs                                       | 10, 100, 1,000 | 33.0, 87.4, 849    | 31.1, 53.0, 327        | 25.3, 49.4, 252        | −23%, −43%, −70%    | Yes              |
| Removal, µs                                         | 10, 100, 1,000 | 31.8, 124, 2,469   | 36.3, 64.5, 371        | 26.1, 58.9, 347        | −18%, −53%, −86%    | Yes              |
| Reorder, µs per item                                | 10, 100, 1,000 | 2.63, 3.77, 12.6   | 2.59, 3.77, 12.5       | 2.46, 3.57, 12.4       | −6% to −1%          | No               |
| Mount, µs per item                                  | 10, 100, 1,000 | 21.0, 22.0, 23.4   | 22.2, 20.9, 25.9       | 21.4, 18.7, 23.5       | +2%, −15%, +1%      | No               |
| Teardown, µs per item                               | 10, 100, 1,000 | 11.9, 13.4, 12.9   | 15.3, 15.6, 16.8       | 11.5, 11.4, 12.8       | −4%, −15%, −2%      | No               |
| JavaScript heap per item, bytes                     | 10, 100, 1,000 | 2,636, 922, 809    | 3,221, 1,693, 1,487    | 3,761, 1,012, 897      | +43%, +10%, +11%    | Yes              |
| Bare container per item, bytes                      | 10, 100, 1,000 | 263, 69, 55        |                        | 596, 150, 119          | +126%, +117%, +119% | Yes              |
| Bytes allocated per label update                    | 10, 100, 1,000 | 268, 652, 4,253    | 86, 86, 85             | 57, 57, 57             | −79% to −99%        | Yes              |
| Bytes allocated per done toggle                     | 10, 100, 1,000 | 247, 631, 4,232    | 114, 138, 138          | 86, 110, 110           | −65% to −97%        | Yes              |
| Bytes allocated per bulk label update of every item | 10, 100, 1,000 | 701, 4,757, 44,983 | 983, 9,044, 89,559     | 1,716, 16,535, 171,439 | +145% to +281%      | Yes              |
| Bytes allocated per bulk done update of every item  | 10, 100, 1,000 | 678, 4,709, 45,202 | 1,446, 16,291, 162,641 | 1,507, 16,557, 171,589 | +122% to +280%      | Yes              |

Selection reads, open-count derivations and summary writes per operation,
from the counted pages (N items, k items changed):

| Operation                                 | Shared map                           | Store per item | Keyed collection |
| ----------------------------------------- | ------------------------------------ | -------------- | ---------------- |
| Label update                              | 2N + 1 reads, 1 derivation, 0 writes | 1, 0, 0        | 1, 0, 0          |
| Done toggle                               | 2N + 1, 1, 1                         | 2, 1, 1        | 2, 1, 1          |
| Bulk label update of k items              | 2N + 1, 1, 0                         | k, 0, 0        | k, 0, 0          |
| Bulk done update of k items               | 2N + 1, 1, 1                         | 2k, k, k       | k + 1, 1, 1      |
| Insertion                                 | 2N + 4, 1, 1                         | 5, 1, 1        | 3, 1, 1          |
| Removal                                   | 2N + 2, 1, 1                         | 2, 1, 1        | 1, 1, 1          |
| Reorder                                   | 2, 1, 0                              | 2, 1, 0        | 1, 1, 0          |
| Remove and reinsert one key in one action |                                      |                | 3, 1, 1          |

Row DOM effects were identical between the keyed collection and the shared map
at every step and size.

**Reading the costs.**

- **Item updates** cost what store per item costs, about 3 to 5 µs at every
  size, and allocate 57 bytes, with no derivation for a field the open count
  does not read.
- **Bulk actions** commit once: one derivation and at most one summary write
  for a bulk done update of every item, whose time matches the shared map's,
  against about 1,000 of each and 6× the time with store per item at 1,000
  items. Bulk updates allocate about 170 bytes per changed item, nearly four
  times the shared map's single map copy: one new record, its undo-log entry
  and its before and after records for delivery.
- **The open count stays linear** when it is evaluated: a done toggle at 1,000
  items costs 45.9 µs, against 31.5 µs with store per item, because the count
  reads each item through the collection's map.
- **Memory.** The bare collection holds about 65 bytes more per item than the
  shared map's store (an incarnation per item); mounted rows add the same
  per-row cost in both, about 755 to 780 bytes at 1,000 items and 850 to 860
  at 100, so item subscriptions cost no more than the shared map's.
- **Membership changes** are cheaper than the shared map's, which copies the
  map of items on every insertion and removal.

### Stage 5: shipping path (2026-10-25)

Variant 5, the shipping collection imported from `@canonic2/lattice/collection`,
against variant 1, the shared map, with variant 3, store per item, and variant
4, the prototype, as references. Medians across pages of each page's median;
"meaningful" uses the stage 1 rule against the shared map.

| Measure                                             | Items          | Shared map         | Store per item    | Prototype              | Shipping               | Shipping vs shared map | Confirmation: shared map, shipping |
| --------------------------------------------------- | -------------- | ------------------ | ----------------- | ---------------------- | ---------------------- | ---------------------- | ---------------------------------- |
| Label update, µs                                    | 10, 100, 1,000 | 3.84, 21.3, 341    | 2.79, 3.22, 5.35  | 2.89, 3.19, 5.06       | 2.83, 3.19, 4.96       | −26%, −85%, −99%       | 3.87, 21.8, 342; 3.00, 3.09, 4.89  |
| Done toggle, µs                                     | 10, 100, 1,000 | 3.69, 21.7, 343    | 2.87, 4.87, 32.1  | 3.13, 6.13, 45.1       | 3.11, 6.22, 45.9       | −16%, −71%, −87%       | 3.74, 22.1, 343; 3.25, 6.13, 46.8  |
| Bulk label update of 10 items, µs                   | 10, 100, 1,000 | 20.1, 39.5, 360    | 20.0, 21.6, 21.8  | 21.9, 22.5, 23.1       | 21.6, 22.5, 23.0       | +8%, −43%, −94%        | 20.0, 39.7, 362; 22.7, 22.0, 23.5  |
| Bulk label update of every item, µs                 | 10, 100, 1,000 | 19.8, 226, 3,670   | 19.6, 205, 3,920  | 21.2, 213, 3,555       | 21.0, 216, 3,470       | +6%, −4%, −5%          | 19.9, 219, 3,615; 22.2, 214, 3,330 |
| Bulk done update of 10 items, µs                    | 10, 100, 1,000 | 20.5, 40.0, 358    | 27.7, 43.1, 281   | 22.6, 26.2, 64.4       | 22.5, 26.4, 63.9       | +10%, −34%, −82%       | 20.7, 40.2, 357; 23.2, 26.3, 64.9  |
| Bulk done update of every item, µs                  | 10, 100, 1,000 | 20.4, 235, 3,915   | 27.4, 456, 25,075 | 22.1, 230, 3,670       | 22.4, 237, 3,470       | +10%, +1%, −11%        | 20.3, 236, 3,945; 23.0, 232, 3,590 |
| Insertion, µs                                       | 10, 100, 1,000 | 34.0, 89.2, 860    | 29.1, 55.7, 332   | 27.0, 49.0, 255        | 25.1, 50.1, 253        | −26%, −44%, −71%       | 34.2, 90.1, 892; 29.1, 49.0, 260   |
| Removal, µs                                         | 10, 100, 1,000 | 33.2, 127, 2,472   | 35.2, 67.9, 385   | 27.6, 58.3, 344        | 26.5, 58.8, 345        | −20%, −54%, −86%       | 33.4, 127, 2,532; 29.0, 58.0, 353  |
| Reorder, µs per item                                | 10, 100, 1,000 | 2.67, 3.76, 12.7   | 2.61, 3.93, 12.7  | 2.48, 3.52, 12.5       | 2.45, 3.60, 12.4       | −8%, −4%, −2%          | 2.63, 3.78, 12.8; 2.54, 3.57, 12.7 |
| Mount, µs per item                                  | 10, 100, 1,000 | 20.3, 20.4, 23.4   | 22.4, 22.3, 26.3  | 21.5, 18.9, 22.9       | 21.5, 19.1, 22.7       | +6%, −7%, −3%          | 20.9, 22.3, 23.3; 22.4, 18.8, 23.1 |
| Teardown, µs per item                               | 10, 100, 1,000 | 11.6, 11.6, 13.1   | 15.2, 16.4, 17.1  | 11.8, 11.5, 13.0       | 11.7, 11.5, 13.1       | +1%, −1%, +1%          | 11.7, 13.3, 12.9; 12.1, 11.4, 13.1 |
| JavaScript heap per item, bytes                     | 10, 100, 1,000 | 3,184, 922, 809    |                   | 3,761, 1,012, 897      | 3,845, 1,085, 898      | +21%, +18%, +11%       |                                    |
| Bare container per item, bytes                      | 10, 100, 1,000 | 271, 70, 55        |                   | 604, 151, 119          | 614, 152, 119          | +127%, +117%, +119%    |                                    |
| Bytes allocated per label update                    | 10, 100, 1,000 | 268, 652, 4,253    |                   | 57, 57, 57             | 57, 57, 57             | −79% to −99%           |                                    |
| Bytes allocated per done toggle                     | 10, 100, 1,000 | 247, 632, 4,232    |                   | 86, 110, 110           | 85, 110, 110           | −65% to −97%           |                                    |
| Bytes allocated per bulk label update of every item | 10, 100, 1,000 | 700, 4,762, 44,987 |                   | 1,718, 16,568, 171,318 | 1,721, 16,558, 171,273 | +146% to +281%         |                                    |
| Bytes allocated per bulk done update of every item  | 10, 100, 1,000 | 678, 4,716, 44,940 |                   | 1,507, 16,568, 171,270 | 1,433, 16,581, 171,329 | +111% to +281%         |                                    |

The shipping collection matches the prototype within run-to-run variation in
every timing cell, and to the byte in allocations. Heap per item at 100 items
is 73 bytes above the prototype, about the 61-byte interquartile range of that
cell; at 1,000 items they are equal.

**Garbage collection**, from the traced runs: medians across 10 pages, per run
of 20,000 label updates, 20,000 done toggles, or a bulk done update of every
item repeated 20,000 ÷ N times (2,000, 200 and 20 calls at 10, 100 and 1,000
items, each about 3.4 MB of allocation with the shipping collection).

| Workload                       | Items          | Shared map: collections, ms | Shipping: collections, ms |
| ------------------------------ | -------------- | --------------------------- | ------------------------- |
| Label updates                  | 10, 100, 1,000 | 1, 1.3; 3, 2.1; 21, 9.0     | 1, 1.3; 1, 1.2; 0, 0      |
| Done toggles                   | 10, 100, 1,000 | 1, 0.5; 3, 1.1; 10, 4.2     | 0, 0; 0, 0; 0, 0          |
| Bulk done update of every item | 10, 100, 1,000 | 1, 0.4; 0, 0; 0, 0          | 1, 0.4; 1, 0.4; 0, 0      |

Every collection was a minor one; no run in either variant had a major
collection. The shipping collection's bulk updates triggered at most one minor
collection of about 0.4 ms per 3.4 MB run; single-item updates, which allocate
57 to 110 bytes, triggered none at 1,000 items, where the shared map's copy of
the map triggered one collection every 1,000 to 2,000 updates.

Counted pages: row DOM effects of the shipping collection were identical to
the shared map's at every step and size, and every count, including `read`s,
open-count derivations and summary writes, was identical to the prototype's.

## Correctness

### Stage 6: replacement diagnosis verification boundary

The runtime source is unchanged, so passing affected tests are reused. Every
new case checks full order/value outcomes, copied source-record release,
positive live-owner counts and zero disposed owners. The repaired parser is
also exercised against all earlier Node/Chromium commit/rollback/churn heaps;
known initialized objects must be found before any zero at disposal can pass.
This validates the check against captures whose IDs actually differ, rather
than assuming an improved capture transport makes IDs stable. Twenty affected
allocation profiles complete full value/error oracles after sampling. No new
timing correctness or unrelated allocation result is claimed.

### Stage 6: corrected-candidate continuation boundaries

Passing affected correctness and packed-consumer tests are reused on the
unchanged corrected collection source; the preflight separately checks JSDoc.
Every replacement/rollback lifetime verifies exact current values/order and
snapshot identities before returning, outside snapshot capture. Eight cases
complete the declared warmup, release and churn transitions with no functional
oracle failure. The selected typed-array calibration checks byte lengths and
both weak identities. Chromium's stable heap identities support the checked
private-owner release claims. Node's cross-snapshot ID operations and hidden
cohort classifiers are invalid for that proof, even though exposed WeakRefs
clear. Their apparent survivors are explicitly excluded from leak conclusions.
No timing or allocation correctness result is claimed for unstarted phases.

### Stage 6: integrated candidate behavioral boundaries

The final candidate keeps the exact executable planner/application logic of
the measured direct prototype; private/public JSDoc and the normal package
entry are integrated. New unit/browser cases and consumer fixtures cover the
reviewed ownership table in the approved plan. Separate counted lab probes in
Node and Chromium checked equivalent delivery counts, rollback, getter guards,
replacement and lifecycle behavior before timings. Every completed timed action
was checked against the full oracle outside its clock. Behavioral correctness
does not close the incomplete retained-object or performance acceptance gates.

### Stage 6: matched diagnostic scope

The Node and Chromium lab semantic probes exercise ordinary batch reads,
exact nested rollback, replacement/subscription lifetimes, abort cleanup,
reentrant delivery/registration/errors, read-once guarded getters and stable
anchor groups against the exact B0/B1 source pair. Small counted smoke checks
all four membership families and all three small controls. D0 is used only for
construction, existing subscription setup smoke and the constructor trace;
its added method body is never called. Timed constructors check size, immutable
keys and canonical initial values outside clocks; every small action checks
full output/identities before public reset. Every large one-shot checks the
full independent oracle, ordering, values, survivor/replacement identities and
cleanup after action timing. No failing product assertion or cleanup error is
reported. Node C01 has no completed primary timing result, and no package
correctness/consumer suite is implied by these lab probes.

### Stage 6: direct integration smoke scope

The Node lab's existing semantic probes completed for both factories, including
exact nested rollback identity, incarnation replacement, abort/listener cleanup,
reentrant delivery/errors, guarded/read-once getters and stable anchor groups.
Separate counted preflight covered all seven chosen action cells and equivalent
delivery counts. Completed setup samples validated collection sizes and frozen
keys outside the clocks. The one append warmup pair validated full values,
ordering and survivor identities after action/abort. The interrupted fixture
and unrun Chromium/profile phases have no correctness result. These are lab
probes, not a run of the package's correctness or consumer suites.

### Stage 6: final accept-or-discard stage

Completed Node/Chromium semantic, counted-delivery and post-action probes
observed no product correctness failure. They checked exact order/values,
frozen/stable old keys, surviving record identities, replacements, rollback,
abort, errors and subscription delivery. Deadline-interrupted work remains
incomplete. The discarded implementation's behavior tests and source remain
in its recorded Git revision. Current regression coverage retains regular
collection/store cleanup and packed-package import boundaries: the package
contains no experimental runtime and the unsupported subpath is refused.
Consumer inference/invalid-call fixtures cover the remaining public entries.

### Stage 6: independent memory and regular cleanup

Semantic/counting preflights used the actual experimental export and frozen B0
for all selected workload groups, outside profiling. Completed action profiles
passed post-window value/order/identity checks. Lifetime profiles checked their
bounded results after sampling stopped. Retained commit/rollback calls verified
canonical outcomes and rollback identities before dropping temporary oracles.
Caller holds existed until explicitly released. All four weak-reference groups
released their targeted objects in all five observations, and all four churn
groups completed 100 cycles. No product correctness error stopped the phase.
The warming/cancellation defects are runner corrections; affected data is not
claimed as acceptance.

The independent regular collection/store fixes have work-count, original-error,
live-signal detach and ended-callback regression cases in their owning test
files. This is not a claim that the broader package/type/browser suites ran.

### Stage 6: experimental package checkpoint

The lab adapter loads the frozen baseline and actual experimental package
entry. Shared/candidate semantic probes, counted preflights and all executed
post-timer checks passed in both initial/corrected teardown reproductions and
the two admission cases. Checks cover frozen/stable keys, unchanged old arrays,
record values/order, survivors, replacement identities, expected delivery
counts, errors and lifecycle ownership. No correctness failure stopped a set.
The corrected abort callback was exercised through the actual package export;
live-signal removal paths remain separate from once-dispatch cleanup.

The authored Vitest suite and packed/type fixtures map additional checkpoint
contracts to runnable tests. This record does not treat authored or partial
test coverage as a full executed package suite. Release by forced-GC probes,
retained memory, allocation and cross-engine acceptance remain unverified.

### Stage 6: eight-minute diagnostic amendment (2026-10-26 UTC)

The separate validation process passed the shared and candidate semantic
probes, the exact Q03 lifecycle, and counted small-size append/removal/
replacement probes with equivalent delivery counts. The expected-error and
owned-child cancellation probes completed before the measurement set.
All measured setup/removal lifetimes and all 54 bulk warmup/measured actions
passed the executed post-timer checks: exact order/values, stable/frozen
keys, unchanged old keys, survivor references, replacement record identities
and no reported delivery errors. Full removal and replacement ended old
item watchers at commit; retained append watchers ended at owned abort.

No correctness or runner failure occurred. All child processes were closed,
with no stderr or cleanup errors. No forced-GC release, allocation, retained
heap, churn or cross-engine acceptance was performed; GC tracing alone does
not establish those gates.

### Stage 6: approved reduced run (2026-10-26 UTC)

Both variants passed the exercised semantic probes (seven shared probes and
three additional candidate validation/anchor probes), repeated in the isolated
validation realm before each timing block. All actions in Q01–Q03 passed
post-timer exact order/value, frozen/stable keys, unchanged old keys, survivor
identity, replacement-incarnation and reported-error checks. No correctness
failure ended this run.

Separate counted preflights, repeated over three restored actions, matched:
Q01 delivered nothing without subscriptions; Q02 delivered membership once,
one updated item's read/callback and one collection-wide read, suppressing its
unchanged size callback; Q03 delivered membership and size once with no item
read/callback. All counted work was outside acceptance action timings.
Large workloads and the seven named small controls did not reach preflight
or measurement in this run. Forced-GC release, retained heap, churn and
allocation acceptance were not run. Exercised semantic cleanup does not
establish those unmeasured memory gates.

### Stage 6: resumed evaluation (2026-10-26 UTC)

No candidate correctness failure was observed. Both resumed calibration
processes passed their common and bulk-specific probes. Each completed
corrected timing cell passed fresh-fixture probes and three restored-fixture
comparisons for each variant before sampling. Restored delivery counts matched
between variants, including item readers and callbacks, membership and
collection-wide reads/callbacks. Exact values, order and survivor identities
were checked after every measured action; the semantic probe sequence ran
before each measured block. Restoration used public operations outside the
timer and re-established subscriptions for replaced original incarnations.

That is bounded observed coverage, not completed acceptance. Forced-GC release,
retained heap and churn were not run; the interrupted cell has no complete
saved sample and is not passing coverage. The initial extended-calibration
attempt was classified as a runner procedure defect, not a product failure.

### Stage 6: smoke correctness (2026-10-26 UTC)

Both variants passed the common probes in Node and Chromium: successive reads
inside ordinary batches; frozen old arrays and survivor identities; exact
nested and outer rollback; same-key incarnation replacement and ending old
watchers; transient incarnation/no-op batches; aborted subscriptions staying
ended after rollback; committed removal detaching a live signal listener;
queued reentrant writes and late registration; failed initial callbacks and
batch subscription refusal; abort during delivery; failed callbacks retaining
their last successful value; isolated read and run errors.

B1 also passed atomic rejection of missing/duplicate/conflicting keys, null
change arrays and invalid items; invalid calls caught after earlier outer
writes; empty input preserving keys identity; read-once item getters observing
pre-call state with reentrant writes refused; throwing-getter atomicity; and
stable insertion across multiple surviving-anchor groups and appends.

The independent oracle and counted smoke comparisons passed in both engines
for all exercised workloads and sizes. No correctness failure or runner defect
was observed. This is bounded smoke coverage, not completed correctness
acceptance: forced-GC release/retention, full small-operation coverage and the
remaining measured-sample probes were not run after calibration stopped.
Chromium reported no page errors; browser cleanup completed.

### Initial comparison

| Variant         | Counted pages | Failed checks |
| --------------- | ------------- | ------------- |
| 0. Hand-written | 9             | None          |
| 1. Lattice      | 9             | None          |

Both passed, at 10, 100 and 1,000 items: rows matched the items after mount;
an update changed exactly one row; a reorder reversed the rows by moving them
without creating or removing any; after teardown no row remained, every
observer was disconnected and listener and node counts returned to their
baseline.

### Stage 3

| Variant           | Counted pages | Failed checks |
| ----------------- | ------------- | ------------- |
| 1. Shared map     | 9             | None          |
| 3. Store per item | 9             | None          |

Both passed, at 10, 100 and 1,000 items, after mount and after every
operation: rows and the open count matched the bench's own model; every
remaining row kept its element across label, done and bulk updates,
insertion, removal and reorder, and a reorder created or removed no row; each
of `setLabel`, `setDone`, `setLabels` and `remove` on a removed id threw a
`TypeError` and caused no DOM effect, derivation or row callback; after
teardown no row or summary remained, every observer was disconnected and
listener and node counts returned to their baseline.

The first smoke run failed teardown in both variants: the bench's identity
snapshot still held the rows. Clearing it on unmount fixed the runner; no
variant changed.

### Stage 3 follow-up

Both variants passed every check on 9 counted pages each: after each bulk
done update the rows and open count matched the bench's model and every row
kept its element, and after teardown listener and node counts returned to
their baseline.

### Stage 4

| Variant             | Counted pages | Failed checks |
| ------------------- | ------------- | ------------- |
| 1. Shared map       | 9             | None          |
| 3. Store per item   | 9             | None          |
| 4. Keyed collection | 9             | None          |

All three passed the stage 3 checks after every operation, including the bulk
done updates. The keyed collection also passed, at every size: at most one
open-count derivation and summary write per action, and none for label
actions; no summary write for a reorder that left the count unchanged; and a
key removed and reinserted in one action getting a new row in its place while
every other row kept its element.

The prototype's own tests, run before the measured sets, cover the proposal's
contract: identity of keys and records; function patches read once and every
write rejected while a patch is resolved; insertion with and without `before`;
reorder validation; the membership, item, collection-wide delivery order;
subscriptions created during delivery skipping the commit in progress;
collection-wide values re-evaluated on reorder, suppressed by `equals`, and
not evaluated for unrelated fields; one notification per subscription for a
bulk batch; transactional removal rolled back by the batch and by nested
savepoints with subscriptions restored, a savepoint's rolled-back update
notifying nothing, and a subscription whose signal aborted inside a
rolled-back batch staying ended; key reuse inside a batch, across commits and
during delivery, with queued notifications for the old incarnation dropped;
and delivery errors passed to `report`. All pass.

### Stage 5

| Variant                | Counted pages | Failed checks |
| ---------------------- | ------------- | ------------- |
| 1. Shared map          | 9             | None          |
| 4. Prototype           | 9             | None          |
| 5. Shipping collection | 9             | None          |

The shipping collection passed every stage 4 check at every size, including
the summary limits and key reuse. Before the runs, the package's own tests of
the shipping implementation passed: the collection's unit tests, which cover
the contract's acceptance areas, the store's unit tests after the shared
reporter moved, the Node import test of the new entry point, and the
packed-archive test with the collection's consumer type fixtures in both
environments.

## Threats to validity

### Stage 6: replacement diagnosis limits

The detailed pre-action heap forces GC and changes object aging. A separate
unsampled replacement interval therefore preserves the original reproduction;
its template-mode exceedance remains visible. The fixture's generated labels
and equality oracle affect string representation. Identical heap label targets
are compared only within a capture, and ID-free logical-owner counts support
disposal across captures. Flat-input results apply to that verified input mode;
they cannot erase template-mode application bytes or establish every workload's
budget. The exact VM shortcut/tiering cause is not separately profiled. Five
allocation samples have exploratory maxima, but their full conservative envelopes
fit the two existing allocation gates. No host-qualified timing or broad
acceptance campaign is performed.

### Stage 6: corrected-candidate continuation limits

No quiet host window was admitted, so there is no new primary performance
comparison. Retention ownership analysis proceeds independently as authorized.
Node captures expose changing IDs for an object whose JavaScript identity stays
fixed; baseline-ID filtering and intersection cannot establish hidden-object
release. Its incarnation/subscription classifier also fails the live-state
presence check. Byte ownership within individual heaps and actual WeakRefs
are independent observations, but they do not repair the full private-identity
requirement. Code/shape metadata is separated before reporting application
storage, and record-owned string wrappers remain application storage. The
captured Node live-byte failure has no independent repetition beyond the
declared sequence; rollback and Chromium differ. The byte representation cost
is observed and owned, while its intrinsic/repeatable cause remains unresolved.
The planned gate stop leaves every allocation confirmation unmeasured. Offline
attribution/reporting exceeds its allowance; no measurement ceiling/count is
extended as a consequence. Preserve all those limits rather than inferring
acceptance from earlier prototype timings or from Chromium alone.

### Stage 6: final integrated acceptance limits

Initial host load was substantially higher than the prior matched diagnostic
and rose to 275.95/138.34/76.81 by the end;
absolute setup and several small-operation samples have wide IQRs/outliers.
The fixed gate still applies, but these observations do not isolate intrinsic
method/property cost. The planned two-failure stop leaves most setup replicas
unmeasured. Fifteen-block and seven-action p95 equal their maxima and remain
exploratory; partial/cap-limited prepend samples cannot establish acceptance.
The memory positive control assumed four-byte array slots and failed its
predeclared size check on an eight-byte-slot Node build. Offline backing-store
inspection explains that fixture defect without retroactively passing it.
No full-replacement application-retention, matched code-footprint delta, churn
or allocation confirmation exists for this final package-path run. Older
prototype/other-candidate observations are not substitutes. Reporting artifacts
completed within the run; research reconciliation exceeded reporting headroom,
without extending checks or measurements. All limitations remain rejection
grounds under this version's rule rather than invitations to another run.

### Stage 6: matched diagnostic limits

This closes the specific measurement dispute: matched retained chunks and
valid per-block timing do not reproduce a stable near-doubling. It does not
assign the earlier ratio wholly to GC/chunk size or establish a precise CPU
cost of the new method/property. Native allocation layout, code sites, JIT
conditioning and shared-host load remain confounders. D0 controls a small
unused method/property, not identical generated machine code or every closure
capture effect. Actual code footprints differ and no code-byte budget was
waived. Browser realms are independent heaps/globals, not independent machines.

Forty-block constructor p95 is limited evidence; fifteen-block p95 equals the
maximum and cannot independently establish tail regressions. All unfavorable
replicas/maxima remain visible, especially Chromium replica 2 subscribed setup.
Sampled constructor bytes overlap across five profiles; neither the tiny exact
extra bytes nor an allocation reduction is resolved. C02's extra allocation
is a complete-action estimate, not an individual object's measured size.
Profiles include VM/probe storage; allocation-site attribution can collapse
inlined helpers into the factory. CPU traces include Inspector overhead and
different counts normalized to 20 ms; the GC observer supplies no resolved
event durations despite GC CPU samples. No attribution is fabricated from
that absence. No instrumentation rerun or extra profile was started.

Large diagnostics have one cold pair at N=1,000; no percentiles, repeated gain,
N=10,000 result or complete allocation/lifetime/retention/churn acceptance
follows. Constructor sampling and previous retained-object snapshots measure
different properties/revisions. The proposed application-graph versus persistent
runtime-footprint criterion remains a proposal, with the original whole-heap
failure preserved. No new retained-heap conclusion is asserted for direct B1.

### Stage 6: direct integration partial evidence

Offline review subsequently found most setup blocks fail the original 20 ms
minimum: calibration had accumulated different-sized trials then reused only
the last count. The full constructor audit in Method preserves the exact
counts. This is a runner defect, not permission to drop unfavorable samples or
pass a gate. Source review finds only a method declaration/property added to
the factory body; its allocation/JIT/shape consequences remain unmeasured.

The chosen repetition schedule was too expensive at the observed 10,000-item
control costs; the 3–6-minute estimate was wrong. Calibration should have been
used to bound the full schedule before starting large warmup blocks. Preserved
setup cells are complete; the action comparison is not. Constructor input/result
chunks differ in size because repetitions were independently calibrated, so GC
pressure and p95 smoothing differ. Concurrent compilation and large IQRs make
tail attribution unresolved. No code/layout change can be credited with removing
the historical regression. A proposed graph-based retention criterion is not
an approved replacement for the historical whole-heap test; no new memory pass
follows from it or from the prior graph-release diagnostic.

### Stage 6: final acceptance limits

Stable no-product and positive controls support the combined harness correction;
they do not attribute the remaining full-replacement positive/negative shifts
to live collection objects. The matched heap pair and most corrected churn
groups were not reached. No leak-freedom or retained-byte acceptance claim
follows. Sampled allocations are estimates. Setup p95 is a maximum of fifteen
block averages on a shared host, not an individual-operation latency or proof
of a specific constructor defect. Required failed and missing comparisons
remain failed or missing; passing means, engines or earlier runs cannot waive
them. Rejection follows the owner's fallback rule without another expansion.

The frozen control precedes the independent once-abort cleanup correction;
teardown comparisons therefore include that separate benefit. Total-action
clocks exclude abort. No corrected regular-entry shipping integration was
performed, because this candidate was rejected rather than adopted.

### Stage 6: independent memory evidence

Five allocation profiles are sampled estimates, not exact totals. Observed
profile ranges are descriptive uncertainty, not confidence intervals. C02's
range is beyond its gate; Q03 crosses its gate and is inconclusive. Narrow
passing margins, including C03, need independent confirmation. Flawed fresh
Q/C action profiles remain exploratory regardless of outcome; no unfavorable
cell was silently dropped or candidate tuning performed mid-run.

Forced-GC used-heap readings include runtime, compiled code and probe state.
Warmup did not settle all helpers/branches before the first empty snapshot.
Both variants show residuals in empty controls and further stepwise churn
growth. Byte-budget failures are therefore not attributable solely to product
retention. Targeted weak-reference release cannot waive them or establish that
every collection object is unreachable. No retainer snapshot isolates the
residue here. All failed replicates and spikes remain evidence; a bounded
calibration/retainer investigation is required before another large acceptance
set. Do not keep warming or collecting until a favorable baseline appears.

Setup CPU profiles contain GC samples while the event observer returned no
entries before capture, so its event/time correlation is unavailable. Shared
profile-order effects and near-equal setup allocations do not explain the
original admission p95 tail. High host load and profiling perturbation remain
limits. Memory completion does not imply timing admission or API acceptance.

### Stage 6: checkpoint admission and cleanup attribution

The repeated abort improvement is supported by a work-count reproduction,
before/after source correction, profiles without observed GC during abort,
pre-GC controls and three-process repetition through the package entry. The
mechanism is shared redundant once-listener removal; the experiment does not
fully isolate the lower-level reasons native searches cost more in the old
candidate heap than the baseline heap. The short corrected CPU profile has
too few samples for precise component attribution.

Admission setup p95 is the maximum of fifteen block averages, not a robust
individual-operation percentile. Load rose from 10.13 to 21.39 during that
set. The setup regression is a gate failure with unresolved cause, not proof
of a persistent intrinsic constructor penalty and not a reason to waive the
gate. Missing cases, memory evidence and confirmation cannot be inferred from
the targeted abort improvement. The default entry deliberately retains the
accepted implementation until a separate acceptance decision.

### Stage 6: eight-minute diagnostic amendment

Initial load average 60 on 16 logical CPUs makes host contention a material
limit. Strict interleaving and independent processes preserve that variation;
they do not remove it. The primary setup p95 is a percentile of 200 block
averages of 100 setups, not the latency of an individual setup. The selected
ten-block bootstrap cannot characterize pauses correlated over longer runs;
its wide intervals prevent confident causal attribution. Do not discard
process 1's failure, process 3's maximum or the original failed result.

Bulk has seven actions per variant in one process per operation. Its large
median gains exceed the observed IQRs but remain diagnostic, without an
independent confirmation, allocation or retention evidence, small-operation
coverage, mixed actions or Chromium measurements in this amendment. Candidate
append abort is slower and untraced; there is no supported cause for that
regression. The separately instrumented setup passes cannot explain a
different large-workload tail. No complete-lifetime median was measured.

### Stage 6: reduced run tail evidence

The stop is a failure of the agreed acceptance statistic, not proof of a
persistent candidate-specific setup regression. Fifteen block means make p95
the maximum; the setup failure is dominated by one candidate block while
setup medians are nearly equal. A different control block also has a setup
tail. Shared host load, runtime/heap conditioning and GC remain possible
causes; none was instrumented or removed from decision samples. Timer
resolution was not separately calibrated in this run, and the optional
GC-attribution diagnostic was not collected. The original and new unfavorable
samples remain visible. No independent confirmation was performed after the
gate-triggered stop, as required by the owner's latest instruction.

The large fresh-block path and independent large-case counts were not reached.
Their implementation therefore provides no performance or acceptance evidence.
The executed Q01–Q03 cold paths have contiguous setup/action/abort boundaries,
with validation after the timer. Future use of the fresh-block helper must
distinguish its recorded reference-capture gap from a clean complete-lifetime
boundary; its unexercised span includes that gap and cannot be claimed as the
planned clean lifetime measurement without correction before use.

- The resumed stage 6 initial set is incomplete: 12/184 paired cells, one
  process and no independent confirmation. The initial unnecessary extension
  to full fresh-fixture calibration consumed most of the budget; preserve
  that runner limitation when assessing what remains unmeasured.
- Warm reset uses public operations outside timing, retains initial model
  references for identity comparison and re-subscribes replaced incarnations.
  Its heap/cache/registration history differs from a newly constructed
  collection. Fresh correctness and cold lifecycle metrics are separate;
  warm action numbers do not establish cold-action or retained-memory costs.
- Main p95 values are quantiles of block means, not single-operation tails.
  At fifteen blocks the reported p95 equals the maximum. The shared host's
  higher load, natural GC and unavailable power state limit interpretation;
  the large control IQR in full removal fails the agreed meaningful-win rule.
- Stage 6 stopped in calibration: one empty-control block in Node, no paired
  candidate timing, no main samples or profiling. The control did not reach the
  approved minimum accumulated duration before its action cap; this is a
  procedure-resolution limit, not a measured failure of the API. Smoke covers
  one change count per size, not the full approved matrix. No performance,
  allocation, leak-freedom or readiness claim follows from those smoke probes.
  Uncollected power state, shared-host load and timer outliers further limit
  interpretation of the calibration value.
- The rows reuse an element with three inputs; heavier rows dilute the
  bindings' share.
- The capability's state shape, items by id replaced immutably, is one common
  design; another shape changes the update cost.
- Headless Chromium on one shared machine under varying background load.
- Stage 3 measures one collection-wide value, a count over one boolean field.
  Derivations over more fields, or bulk updates of the field they read, cost
  more with stores per item and were not measured.
- Stage 3 runs workloads in a fixed order within each page; earlier workloads
  change labels and done states the later ones start from, identically in
  both variants.
- Heap per item at 10 items varies by about 550 bytes between pages in both
  variants, the heap's measurement granularity at that size.
- The follow-up's timing set ran under the heaviest background load of these
  labs, a load average of 49 falling to 28; its deciding ratio, about 5× at
  1,000 items, is far outside the interquartile ranges, and it matches the
  counted derivations.
- Stage 4 measures a lab prototype, tuned once before the run as its
  amendment records; a product implementation must be measured again on the
  shipping path. Under a load average of 9 to 21, some shared-map cells had
  wide interquartile ranges, such as mount at 10 items (IQR 10.8 µs); every
  threshold holds by a margin larger than those ranges.
- Bulk updates allocate nearly four times the shared map's bytes in the
  keyed collection. No threshold covers allocations; their garbage-collection
  cost is inside the measured times only as far as collections happened
  during the timed calls.
- Stage 5's load rose to a load average of 67 late in the confirmation set and
  stayed between 70 and 88 at the start of the counted and memory sets. The
  confirmation agrees with the first timing set, which ran at 6 to 9, in every
  threshold; work counts and heap measures do not depend on load.
- Garbage collections are sparse events: at most a few per traced run, so the
  collection figures show their scale, not a precise rate. Young-generation
  sizing in other hosts and with other application heaps changes when they
  happen.

## Decision and follow-ups

The owner subsequently adopts the bulk API and authorizes aggregate integration.
The supported contract is [atomic membership](../specs/javascript/contracts/collections.md#atomic-bulk-membership).
Earlier withhold/closure decisions below are historical, not pending actions.
No original timing requirement is relabelled as a measured pass.

The [owner closure](#owner-closure-bulk-membership-research) ends this branch as
inconclusive for API acceptance. Independent fixes and the integrated candidate
are preserved. Final timing requirements remain unresolved. No admission,
measurement, correction or promotion is scheduled without a new owner decision.
Earlier decisions below retain their original revision and stage scope.

### Stage 6: preserved candidate and next topic (2026-10-26 UTC)

Keep `f713077…`, its independent cleanup fixes and all measured revisions.
Independent bulk allocation confirmation passes. Keep API acceptance pending
the exact timing-only handoff above. Make no further attempt without an actually
reserved quiet window or another suitable host. No heap work or timing gate
relaxation. The owner explicitly advances incremental aggregate contract design;
that sequencing exception does not adopt bulk membership or authorize new labs.

### Stage 6: quiet-window acceptance recommendation (2026-10-26 UTC)

**Withhold, preserving the implementation.** The confirmed-window run fails
its unchanged host-admission gate before taking any performance samples.
Setup/small/bulk timing requirements remain host-deferred; current-revision
representative bulk allocations remain unmeasured. Correctness, consumer,
retention and resolved small-allocation evidence remain valid in their scope.
No implementation timing or allocation regression is inferred from host load.
No API promotion, acceptance documentation, conditional feature commit,
automatic retry, heap work or further investigation cycle occurs.

### Stage 6: allocation correction recommendation (2026-10-26 UTC)

Keep the concrete planner correction: real avoidable allocation sites were
identified, the focused package-path reproduction brings both problematic
small cells inside their unchanged allowance, and behavior remains covered.
The integrated API remains unaccepted; this focused correction is not another
full acceptance decision. Preserve predecessor evidence, the corrected revision
and independent cleanup fixes. No change to ordinary operations or construction.

Timing remains deferred until the owner agrees a quiet window and competing
workloads are paused. Do not independently pause other sessions, attempt
admission on the busy host, reopen retention or repeat the broad profile set.
Larger-operation evidence retains its measured revision and needs relevant
confirmation before any final adoption claim for the changed planner.

### Stage 6: final remaining-requirements recommendation (2026-10-26 UTC)

**Withhold supported API acceptance.** Direct-append and single-removal
allocation requirements remain unresolved at the approved profile ceiling.
Representative bulk confirmation and setup/small-operation timings are
host-deferred. No attributable regression is established by this final run;
host contention does not reject the implementation. Strong bulk allocation
benefits do not establish the outstanding gates. Keep the integrated method,
correctness coverage and independent cleanup fixes intact and unaccepted.
Preserve all original and new evidence, numerical gates and the closed retention
decision. The approved run is finished; no automatic matrix expansion, further
investigation or additional campaign is started or proposed here.

### Stage 6: corrected retention decision (2026-10-26 UTC)

**Close framework-owned retention as satisfied for revision `9643138…`.** Both
implementations inherit the fixture's string values; the repaired structural
checks find no leftover source-record, incarnation, subscription, key-array,
collection-context or planner owner after the declared release boundaries.
Representation-matched replacement, rollback and churn evidence supports the
lifecycle decision. Persistent code/runtime footprint remains a separate reported
cost, not application retention. No runtime string normalization is warranted.

Preserve the unprobed template result of 246,720 control versus 276,952 candidate
bytes: its observed 30,232-byte delta exceeds the original 24,672-byte allowance.
Do not relabel that numerical result as passing. Its fixture-origin
string representation and probe/GC dependence do not establish unnecessary
framework ownership. This corrected attribution supersedes the retention-based
rejection recommendation for the unchanged revision; it does not accept the API.

Stop heap investigation unless new evidence identifies a framework-owned leak
or excess: obsolete owners or operation scratch surviving their declared
release, or representation-matched excess attributable to framework objects.
Fixture-generated strings, persistent code/runtime footprint and timing noise
alone do not reopen it. Only the allocation and timing requirements in the
[remaining acceptance proposal](#stage-6-remaining-acceptance-only-2026-10-26-utc)
remain. Keep the single candidate, cleanup fixes and passing coverage intact.

### Stage 6: replacement diagnosis conclusion (2026-10-26 UTC)

The reported wrapper excess originates in caller-generated strings and changes
with oracle/GC history. No unnecessary framework owner is found: both paths
copy the same primitive string targets, release source record containers, and
release checked hidden owners at disposal. Repair the measurement's owner
classifier and isolate fixture representation; preserve runtime value-copy
semantics. Controlled flat replacement retention and both affected allocation
gates pass. Keep the original template-mode exceedance and all old evidence.
The bulk API remains unaccepted because that mode and remaining deferred
timing/small-allocation requirements are not established under the all-gates
rule. Keep the one integrated candidate, correctness tests and independent
cleanup fixes intact. Do not restart the full matrix or launch timing work.

### Stage 6: corrected-candidate recommendation (2026-10-26 UTC)

**Recommend reject for this version.** The approved corrected calibration and
Chromium lifecycle findings are useful evidence, while Node's captured live
application-storage gate fails and hidden identity/performance/allocation
requirements remain unresolved or unmeasured. The live excess is owned by
record-label representation; it does not establish an intrinsic planner cause.
The root/identity limitations are measurement findings, not inferred leaks.
Keep the one integrated implementation, all correctness coverage, measured
revisions and independent regular cleanup fixes intact and unaccepted for the
owner's final decision. No discard, new experimental copy, publication or
automatic continuation follows this recommendation.

### Stage 6: final integrated recommendation (2026-10-26 UTC)

**Recommend reject for this version.** Passing behavioral coverage and strong
removal/replacement/mixed timings do not satisfy unresolved append variation,
small-operation limits, repeated setup limits and missing memory/allocation
acceptance. No intrinsic constructor cause or retained application leak is
claimed. The application-retention accounting clarification is approved, while
the original failed whole-heap evidence and all numerical gates remain intact.
Per the owner's explicit instruction, leave the reviewed implementation and
tests intact and unaccepted for the final decision. Preserve frozen revisions,
partial results, research and independent cleanup fixes; no automatic lab
continuation, API publication, discard or next workload is authorized here.

### Stage 6: matched diagnostic implementation recommendation (2026-10-26 UTC)

**Recommend the direct detached membership method and operation-local module
planner as the implementation design. Do not build the reorder bridge.** The
matched evidence does not reproduce the alleged near-doubling; D0 and traces
identify no stable large closure/property penalty or unique larger-code cause
that warrants a private protocol, registry or factory/state rewrite. The direct
call adds an ordinary method/property, and the planner allocates only on use.
Constructor added bytes remain imprecise; complete single-append overhead is
measured within its existing allowance. Bounded one-pair bulk diagnostics retain
the expected benefit. These are concrete design tradeoffs, not adoption gates.

The five-minute diagnostic is complete. Its result resolves that the former
92.8% number cannot justify architectural allocation elimination; it does not
resolve every setup/tail concern or exact per-object allocation. Keep the
unfavorable subscribed setup and exploratory tails as unresolved observations.
No automatic continuation or further acceptance attempt is started. The
experimental API remains unsupported/unadopted, original failed results and
retention proposal remain separate, and shipping behavior/cleanup fixes stay
unchanged. Any later production integration/adoption is a distinct owner
decision, not implied by this recommendation.

### Stage 6: direct integration selected for evaluation (2026-10-26 UTC)

Choose a direct detached membership method and module-scope operation-local
planner over the shared-function/reorder bridge. This avoids a private token,
global dispatch frame, replay protocol and unrelated reorder integration. It
keeps a measurable method/property allocation because its responsibility fits
the factory's existing private-state architecture and it is not a demonstrated
cause of the setup tail. The planner's new returned plan allocation is charged
to the small-operation allowance, not hidden as shared overhead.

The bounded partial run does not establish setup non-regression. Preserve the
unfavorable setup cells, original discarded-candidate result and all memory
budgets. The explicit retention clarification above separates product-graph
release from code/runtime footprint; neither becomes accepted by documentation
alone. No production API or code changes. Review the reduced continuation and
retention criterion as concrete decisions; do not restart a broad matrix.

### Stage 6: explicit shared bulk operation, proposed design (2026-10-26 UTC)

Historical design, superseded by the direct-integration review above. No private
reorder bridge was built. Its complexity is not warranted by the extra closure
allocation without evidence attributing the regression to that allocation.

The owner requires an explicit bulk-membership operation; the earlier public
`update` overload is not the selected API. Recommend a shared module export
`updateMembership(collection, changes)` with a module-level planner, no new
per-instance method and no persistent target registry. This is a design only;
the regular API is unchanged and no broad comparison is started.

Review of the current private-closure implementation rules out simply assigning
a shared function to every returned object: it cannot access the incarnation
map/journal without a state bridge, adds a property, and receiver-based access
would conflict with detached methods. A WeakMap keyed by collection would need
constructor registration and a persistent state carrier or accessor closures.
Migrating every method to such a carrier is a larger ordinary-path change.

Use a private synchronous dispatch frame only while the shared operation runs.
The frame holds the target's original keys function, changes and a claimed bit;
the shared function invokes the target's existing reorder function with a
private module token. Only reorder's existing invalid-array branch recognizes
the token, verifies `frame.keys === view.keys`, claims it once, and enters bulk
planning/application. `view` and its keys function already exist in the current
closure context for collection-wide watching. No constructor registration,
new collection field, per-instance closure or new context value is required.
Valid ordinary reorder keeps its current array guard/body; update/insert/remove/
batch do not gain dispatch checks. The public update signature is unchanged.

Read and validate the target functions once before installing the frame. Require
the dispatch to claim that frame before reporting success; an adapter that
ignores the token must throw rather than silently do nothing. This protocol is
an internal compatibility boundary, not protection against caller-supplied
functions deliberately performing unrelated writes. Keep the existing entry
write guard and guard planning getters before any collection mutation.

The planner performs guarded read-once validation and record copying, returning
one frozen order and call-owned gains/losses. Empty input avoids plan buffers;
single insertion avoids duplicate sets/buckets. Apply the small plan inline in
the cold branch using the existing map/undo/commit machinery, rather than
adding another nested factory helper. Retain only existing journal operands
until outer commit/rollback and queue operands until drain. The target work is
O(N + supplied changes + copied fields), plus existing subscription/native
listener work. Scratch, temporary dispatch and reflective argument storage are
charged to the operation, including the unchanged small-allocation allowance.

The module-local active frame is a temporary owner of a keys function and hence
its collection context. Save/restore the previous frame for nested/reentrant
calls, claim before input getters, and clear target/input references in
`finally`, including invalid input, rollback and delivery errors. It must not
survive a promise, timer, listener or idle module state. No mutable internal
port is exposed. The planner and application introduce no new lifetime owner
after call completion. A shared function's module code and token have one-time
import/code footprint, reported separately from per-collection graphs.

This private protocol needs focused behavior review for target identity,
forwarded native functions, wrong/cross-package targets, replay, cross-collection
nesting, reentrant delivery and every error/rollback path. Collections from a
different package instance or arbitrary adapters must fail rather than partly
apply. Code-layout/JIT effects remain unmeasured; no literal CPU-zero-overhead
claim follows from removing a closure. A shared member/state-registry migration
is not being chosen in parallel.

The new heap evidence addresses the missing ownership finding: measured product
graphs release; surviving residual has module/runtime roots. Keep identity/root
evidence, whole-heap readings and module-code footprint separate in any future
bounded plan, with original gates and budgets unchanged. Review this concrete
mechanism and focused correctness/ownership cases before another comparison;
do not extend or broadly remeasure the discarded candidate.

### Stage 6: operation-local design review (2026-10-26 UTC)

This earlier overload proposal is historical. The owner subsequently retained
an explicit bulk-membership operation; the shared-operation design above is
the current proposal. Its missing full-replacement evidence is supplied by
the later retained-object diagnosis, without changing the earlier results.

The owner requested a short implementation-design review using preserved code
and profiles, without another comparison. The delivered cleanup fixes and the
rejection of the prior implementation remain unchanged. This review introduces
no runtime code, measurement, new allowance or replenished budget.

**Ordinary setup.** The preserved candidate creates one additional bulk-function
closure per factory invocation and adds one returned-object property. That
changes constructor allocation and object shape even for collections that
never use bulk. Its removal/duplicate sets, anchor buckets and planning arrays
are created only inside the operation; they do not explain constructor work.
The saved setup profiles show allocation differences of +0.30%, −0.22% and
+0.09%, and the first CPU-profiled variant is slower in every fresh process.
GC accounts for roughly 20–28% of CPU samples in both variants. These profiles
support shared runtime/conditioning costs, not a causal attribution of the
failed setup tail to the extra closure/property. The tail's cause is unresolved.

**Full-replacement ownership, from source.** Expected references are:

| Objects                                                                 | Owner and release boundary                                                                                                 |
| ----------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| New incarnations, frozen records and current order                      | Collection map/order while the caller holds the collection                                                                 |
| Membership/collection-wide subscriptions and last successful selections | Live collection/subscription; abort releases callback/selection captures, not the collection itself                        |
| Old incarnations, old records and saved keys during a transaction       | Existing undo log, removed/inserted/touched bookkeeping and saved base; outer commit/rollback clears these owners          |
| Pending membership metadata and before/after records                    | Delivery queue; drain clears its references, including reentrant commits                                                   |
| Removed item subscriptions and abort callbacks                          | Ended incarnation releases callbacks/selected values and detaches live signal listeners; its subscription array is emptied |
| Removal/duplicate lookup, buckets and plan arrays                       | Bulk call stack; must not escape into a registry or collection field                                                       |
| Caller-held old keys or records                                         | Caller; immutable snapshots remain valid until that caller releases them                                                   |
| Residual bytes after dropping every product owner                       | Actual objects/root paths not established by existing evidence                                                             |

`resetBatch` clears journal/touched/inserted/removed state and the saved base
keys. `drain` clears queued record/change operands. `end` and `release` clear
item subscriptions and detach their owning signal registrations. These source
boundaries explain intended release, not observed reachability of every object.
The only saved heap snapshots are from the empty-action B0 control. They show
no named per-instance collection closures at that point, but do not identify
the objects behind the full-replacement ±25 KiB shifts. The final matched
replacement heap pair was not reached. Targeted weak-reference release and
stable positive controls cannot fill that missing retainer evidence. No
specific collection leak or runtime-code explanation is established.

**Concrete cheaper design, proposed.** Reuse `update` rather than adding a
member to every collection:

```js
collection.update('a', { done: true }); // existing item form
collection.update({
  // proposed membership form
  remove: ['b'],
  insert: [['b', { label: 'Replacement', done: false }, 'c']],
});
```

The existing item lookup already has a missing-item branch. Only that cold
branch recognizes a non-null changes object with an absent/undefined second
patch; string-key missing-item errors remain errors. Successful ordinary item
updates keep their existing guard/lookup/patch sequence. Keep the original
two parameters; introducing rest arguments, eager defaults or `arguments`
inspection could recreate a cost on ordinary calls.

A module-level planner validates/copies inputs under the existing write guard
and builds one order, O(N + supplied changes + copied fields), excluding the
existing delivery/native-listener costs. The cold branch applies the plan to
the current incarnation map and existing undo/commit machinery. Empty input
needs no plan arrays; one insertion needs no duplicate set or bucket map.
Scratch state exists only for the call. Only the references required by the
existing transaction/delivery contract survive it. Construction and returned
shape gain no property, closure, registry entry or persistent state. Ordinary
insert/remove/reorder/batch implementations do not change. This does not promise
identical machine-code/JIT timing from a larger update function; that remains
a focused verification question, not a reason to repeat the old broad lab.

The alternative of preserving a separate `updateMembership` member cannot
literally leave the returned object unchanged. Lazy getters or a WeakMap
registry still impose a construction/ownership cost; routing bulk through
batch would add dispatch work to ordinary batches. The overload is the
recommended design to review, not an accepted API rename or implementation.

**Gate before further measurement.** Review the overload and concrete cold-path
diff first. Establish behavior/ownership with focused cases, and require a
bounded full-replacement retainer diagnostic distinguishing caller roots,
journal/queue/signal roots and shared runtime/probe storage before another
performance comparison. Preserve all original budgets, thresholds, revisions
and failures; do not extend the discarded candidate or repeat its broad matrix.
No comparison or test/check run was started for this review.

### Stage 6: final decision applied

**Reject for this version; evaluation closed.** The owner's final instruction
requires rejection on failed or inconclusive bounded acceptance. Small append
allocation and measured large-action gains do not establish setup, retained
heap/churn or complete independent confirmation. The experimental runtime,
export and API-specific fixture are discarded; there is one regular collection
implementation. Retain the independent cleanup fixes and their regression
coverage. Supported membership operations retain their documented per-operation
order-copy cost and batch-read semantics. No experimental adoption or further
measurement campaign is pending from this stage.

### Stage 6: attributable append correction, calibration still unresolved

Keep the API experimental and unaccepted. The regular API retains only the
independent cleanup corrections in this commit series; previous working-tree
API changes are outside it. The new single-append diagnostic range fits the
unchanged allocation allowance, with behavioral coverage for the affected
validation/identity/rollback path. This is not a completed timing, lifetime,
retention or full-matrix confirmation of the corrected revision.

The retention instrument detects an intentionally held object, but a shared
first-use footprint and a repeated approximately 32 KiB baseline shift prevent
reliable resolution of the 16 KiB gate. Before a substantial acceptance run,
isolate that shift with matched heap/retainer evidence and stable positive
controls; predeclare workload-specific warmup rather than warming until a
passing baseline appears. The original failures remain recorded. Setup-tail
attribution is still open. No public API adoption follows from these fixes.

### Stage 6: independent memory reported, regular cleanup fixed

**Keep bulk membership experimental and unaccepted.** Independent memory
completed all selected cases without waiving setup admission. Large action
allocations fit the 65% reduction target and large lifetime allocations fit
their no-regression limits. A ten-item single-append bulk call fails the
256-byte additional-allocation allowance; a 100-item single removal remains
inconclusive at the approved five-profile scope.

Retained-heap acceptance remains unestablished: live-overhead numerical failures,
shared post-drop residue and churn growth remain visible. Weak probes show
targeted release, while runtime/probe/code footprint has not been isolated from
product retention. Confirmation remains unrun; neither memory nor setup profiles
override setup p95. No additional samples, relaxed limit or later stage follows.

The accepted regular collection now avoids redundant once-listener searches
on abort; the store avoids redundant detach after an initial callback aborts
then throws. Those independently authorized fixes and regression coverage do
not depend on experimental API adoption. Frozen comparison sources are intact.

Remaining engineering decisions: reduce small bulk-call allocation overhead,
calibrate and attribute retained-heap residuals before another acceptance run,
and resolve setup-tail attribution under controlled conditions. Do not confuse
the completed evidence collection with acceptance of its result.

### Stage 6: reviewable checkpoint, cleanup fixed, admission incomplete

**Implemented/experimental, not accepted.** The checkpoint is in reviewable
source behind `@canonic2/lattice/experimental/collection`, with correctness
and consumer fixtures. The regular collection code/export/instance surface
is unchanged. Original measured baseline, reporter, prototype, initial
checkpoint and corrected checkpoint are separately preserved; measurements
record which revision actually ran.

The attributable abort inefficiency is corrected only in the checkpoint and
the same comparison verifies it. Setup acceptance remains unresolved: the
corrected checkpoint admission failed Q02 setup p95, so the agreed stop rule
prevented allocation/retention and confirmation from starting. Their budgets
were not spent, reset or repurposed. No API adoption or supported-API promise
was introduced, and no later workload stage started.

The remaining decision is how to resolve the setup-tail gate under comparable
host conditions without discarding its failed evidence. Continuing memory
or confirmation despite this admission failure would change the agreed
sequencing/stop rule and requires an explicit amendment; it is not implied
by the prepared checkpoint. Keep the checkpoint available for review while
that question remains open.

### Stage 6: eight-minute diagnostic outcome, prototype unadopted

The approved amendment completed both phases without expanding its matrix
or calibrating repetitions. Setup p95 exceeds its allowance in one of three
fresh processes: **inconsistent/unresolved** under the precommitted label.
The original failed adoption gate is preserved. Bulk append/removal/replacement
deliver the expected diagnostic action benefit, exceeding the halving and
meaningful-gain timing criteria in all three selected cases.

No candidate-specific cause of the setup tail is established. GC overlap is
observed in both instrumented variants, while component differences and
primary process outcomes vary. Large append also exposes a separate slower
candidate abort/teardown, whose cause remains unmeasured. Keep both unfavorable
observations visible; action gains cannot override either.

**Keep the prototype unadopted.** Adoption still lacks resolved setup/cleanup
behavior, allocation/retention acceptance and independent confirmation.
The remaining questions are specific: whether the setup tail persists under
comparable low-contention conditions, and what causes the slower candidate
abort after large append. No further measurement or matrix expansion is
authorized by completing this amendment. No product code was changed, no
shipping comparison was implied, and no later stage was started.

### Stage 6: reduced comparison gate stop, no adoption

**Decision-rule outcome: reject the current candidate for adoption on this
run.** Q03 failed the unchanged setup-p95 allowance by more than either IQR;
the owner explicitly instructed stopping when a clear gate failure made
confirmation unwarranted. No allocation, retention or confirmation budget was
spent. This rejects neither the `updateMembership` API shape nor the bulk
membership engineering direction. There is no owner adoption decision and no
shipping API change.

The specific unresolved question is whether Q03's setup tail is reproducible
and attributable to the candidate's construction/registration footprint, or
to runtime/heap/host variation under the fixed boundary. The separate teardown
tail also remains above its numerical gate but below its IQR uncertainty.
Q01/Q02 passing this set does not retroactively clear their earlier evidence.
Any renewed evaluation needs a bounded, agreed investigation of those tails
before another large comparison; do not quietly exclude the failed block or
relax its p95 gate. The unexercised fresh lifetime helper also needs the
boundary correction described under Threats before use.

All seventeen unfinished selected cases, all Chromium comparisons and every
deferred original cell remain unmeasured in this reduced run. Allocation,
retention, independent confirmation and a shipping comparison remain absent.
Stage 1 stays open; no later stage was started. Retain all raw reports and
the unchanged prototype/control for a future explicitly scoped decision.

### Stage 6: reduced comparison approval

The proposed 20-case amendment is fixed before further runs. It explicitly
changes matrix coverage, large-case fixture/repetition procedure, allocation
action counts and churn coverage while keeping every acceptance threshold and
the original phase ceilings. It retains the unfavorable setup, p95 and noisy
removal cases. Raw partial reports remain unchanged; the saved time audit and
source inspection add explanation, not new performance evidence.

The owner agreed this reduction on 2026-10-26 UTC before measurements,
including stopping on a clear adoption-gate failure. No candidate revision,
adoption, shipping implementation or later stage is implied. The agreed items
are the 20 selected cases,
fresh large-case comparisons with independent calibrated counts, one-action
large allocation profiles with five independent replicates, the two named
100-cycle churn sentinels, and the 25–35/30–50/20–30 minute phase estimates.

### Stage 6: bounded initial evaluation, no adoption

**Decision-rule outcome: inconclusive overall. Recommendation: do not adopt or
ship on this evidence.** The prototype passed the exercised correctness probes
and has substantial median gains in completed membership-heavy cells. It has
not met the complete acceptance rule: the meaningful-win criterion is
unresolved in the sampled 1,000-item full-removal cell, an initial setup result
exceeds its limit, a p95 result exceeds its limit, most timing cells are missing
and no allocation or retained-memory evidence exists.

| Gate                                                         | Evidence and remaining scope                                                                                                                 |
| ------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------- |
| Correctness, identities, rollback and cleanup behavior       | Observed probes passed in Node/Chromium and completed warm/fresh cases; forced-GC release acceptance remains unrun                           |
| 2× membership-only speedup and meaningful difference         | Partial medians only; full-removal control IQR fails the precommitted noise test, other required large cells are missing                     |
| Mixed-action non-regression                                  | Completed medians improve; the 100-item/one-change block-mean p95 exceeds its allowance and needs confirmation                               |
| Cheap operations, setup and teardown                         | Completed action medians fit their allowances; one setup regression exceeds both the allowance and IQRs; unmeasured controls are not passing |
| 65% allocation reduction, small-operation allocation ceiling | Unmeasured                                                                                                                                   |
| Live retained heap, post-teardown growth, churn              | Unmeasured                                                                                                                                   |
| Independent confirmation and shipping comparison             | Unrun; there is no accepted shipping implementation                                                                                          |

The cap amendment does not need another approval: it resolved its intended
calibration problem. The initial comparison nevertheless reached its unchanged
45-minute ceiling after a runner correction, so unfinished cells were stopped
and retained as incomplete. Separate confirmation/profiling budgets were not
used to extend that set and were not exhausted. No source, metric or gate was
changed to convert these results into acceptance.

Concrete remaining work is to confirm/isolate the setup and p95 concerns,
complete the missing timing comparisons with the corrected bounded driver,
and measure allocations plus retained memory/release. Any candidate revision
must keep the original baseline, failed/partial evidence and unchanged gates
visible, with a dated method amendment before measuring. The owner has not
adopted `updateMembership`; public guidance and examples continue to use the
shipping APIs. Stage 1 remains open and no later roadmap stage has started.

### Stage 6: initial inconclusive result and approved continuation

The approved evaluation completed smoke in Node/Chromium, then stopped when
the Node empty-control calibration could not meet the approved 20 ms boundary
within 10,000 actions. No correctness failure was observed in the exercised
probes. There is insufficient evidence to adopt or reject the API on cost.
Retain the unchanged shipping implementation and the local prototype/results;
no shipping API, docs promise, staged change or commit is introduced by this
evaluation.

The concrete next decision is the follow-up calibration proposal under Method:
allow up to 100,000 actions for cheap controls and C <= 1 membership cells,
keeping preparation limits, timing minimum, all gates and wall-time ceilings.
The owner approved that amendment on 2026-10-26; continuation is in progress
under the unchanged time ceilings. Complete calibration and the
remaining lab before considering adoption. Complete this stage, including
shipping-path confirmation if later adopted, before starting incremental
aggregates or any later roadmap stage.

### Initial comparison and earlier decisions

**Outcome.** Binding setup is not the collection's material cost: mounting
costs 2 to 3 µs and about 500 bytes more per item, the price of the bindings
contract's rollback and cleanup guarantees, so a cheaper binding setup is not
worth a comparison on this evidence. Two other costs are material and grow
with the collection:

1. **Single-item updates are linear in the number of items:** 245 µs and
   about 4 KB per update at 1,000 items, against 1.8 µs and 77 bytes for
   hand-written wiring. The cause is the state shape the current API leads to:
   per-item selections on one shared field, and an immutable map copied on
   every update.
2. **Teardown of many subscriptions is quadratic**, because each deactivation
   outside a delivery compacts the subscription array.

**Owner's decision (2026-10-25):**

- **Teardown:** compare bulk compaction, keeping deactivation and the release
  of callbacks and selected values immediate and deferring only the removal of
  empty records, without timers or scheduling. Done in stage 2 and adopted.
- **Updates:** open the collection state-shape question in the state
  management contract, then measure candidates. Field-level selectivity does
  not give item-level selectivity inside one shared field; Lattice is not
  called ready for large collections until that is settled. One store per
  item is a candidate, not yet the rule.

**Stage 3 against its thresholds:**

| Condition                                                             | Result                                                                        |
| --------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| 1. Label update faster at 1,000 items, at most 25% slower at 10       | Holds: −98% at 1,000, −30% at 10                                              |
| 2. Bulk update of every item at most twice the shared map             | Holds: +1%, −12%, +4%                                                         |
| 3. Mount, insertion, removal, reorder and teardown at most 25% slower | Fails for teardown, +30% to +33% at every size; the others hold, at most +12% |
| 4. Memory per item at most 50% higher at 100 and 1,000                | Fails: +84% at both                                                           |
| 5. Done toggle at most 25% slower                                     | Holds: faster at every size                                                   |
| 6. Every correctness check                                            | Holds                                                                         |

**Outcome: mixed.** Conditions 1 and 6 hold and 3 and 4 fail, so by the
plan's rule store per item is not established as the collection shape. It
meets the contract's update requirements with the current API, at about
700 bytes and 4 µs of teardown more per item, a collection-wide derivation
that stays linear in the collection, and bulk updates that are observable
part-way and allocate twice as much. The collection contract stays open until
the owner decides.

**Stage 3 follow-up.** Condition 2 does not hold when a bulk update changes
the field a collection-wide value reads: updating every item's `done` takes
1.35×, 1.84× and 4.96× the shared map's time at 10, 100 and 1,000 items, and
the ratio grows with the collection, because each per-item commit rederives
the value over every item. Updates to a small subset stay within the bar and
become cheaper than the shared map at 1,000 items. The extra memory is almost
entirely the item stores themselves, about 520 bytes each, with 150 to
260 bytes more for the summary binding's per-item subscription. Store per item
therefore costs, beyond stage 3's findings, quadratic bulk updates of any
field a collection-wide value derives from, unless that value is maintained
some other way, and its memory cost is a store's own size. The collection
contract stays open until the owner decides.

**Owner's decision (2026-10-25)** on stage 3 and its follow-up: store per item
is not the default shape. A separate keyed collection capability is the
candidate, evaluated in stage 4.

**Stage 4 against its thresholds:**

| Condition                                                                                            | Result                                                                       |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| 1. Label update at 1,000 items meaningfully faster than the shared map, within 2× of store per item  | Holds: 4.92 µs against 339; 0.94× store per item                             |
| 2. Bulk update of every item within +25% of the shared map, at most one derivation and summary write | Holds: label +7%, −1%, −6%; done +8%, −2%, −8%; one derivation and one write |
| 3. Mount, insertion, removal, reorder and teardown within +25%                                       | Holds: at most +2% (mount at 10 items); the others are faster or equal       |
| 4. Heap per item within +25% at 100 and 1,000 items                                                  | Holds: +10% and +11%                                                         |
| 5. Every correctness check, including the prototype tests                                            | Holds                                                                        |

**Outcome: supported.** Every condition holds. The keyed collection meets the
collection requirements: item updates cost the same at every size, bulk
actions publish once, membership changes are cheaper than the shared map's,
and memory is about 90 bytes more per item. Its costs are bulk-update
allocations, nearly four times the shared map's, and collection-wide values
that remain linear when evaluated. The collection contract stays open until
the owner decides; adopting the proposal means implementing it in the
product with its tests and measuring it again on the shipping path.

**Owner's decision (2026-10-25):** the proposal is accepted for
implementation, within the measured scope. It meets the agreed thresholds and
resolves the trade-off between shared-map updates and per-item stores.

- `keys()` stays a frozen array; measured hot paths use indexed loops.
- Bulk allocation, about 170 bytes per changed item, is an accepted cost of
  this design. It was not a rejection threshold and none is added; allocation
  and garbage-collection effects are measured again through the shipping
  implementation.
- Aggregate costs stay explicit: item-local updates are selective, and
  collection-wide derivations can still scan every item.
- Prototype performance is not established product performance; stage 5
  measures the shipping implementation.
- This settles the primitive, not any application's large-list readiness.
  Selection over 10,000 items and windowing remain separate work.

The [collections contract](../specs/javascript/contracts/collections.md) records the
accepted design, and `src/modules/collection/` implements it.

**Stage 5 against the thresholds**, for the shipping collection:

| Condition                                                                                           | First timing set                                           | Confirmation                                     |
| --------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- | ------------------------------------------------ |
| 1. Label update at 1,000 items meaningfully faster than the shared map, within 2× of store per item | Holds: 4.96 µs against 341; 0.93× store per item           | Holds: 4.89 µs against 342                       |
| 2. Bulk update of every item within +25%, at most one derivation and summary write                  | Holds: label +6%, −4%, −5%; done +10%, +1%, −11%; one each | Holds: label +12%, −2%, −8%; done +13%, −1%, −9% |
| 3. Mount, insertion, removal, reorder and teardown within +25%                                      | Holds: at most +6%                                         | Holds: at most +7%                               |
| 4. Heap per item within +25% at 100 and 1,000 items                                                 | Holds: +18% and +11%                                       |                                                  |
| 5. Every correctness check                                                                          | Holds, with the package tests                              |                                                  |

**Outcome: confirmed.** The shipping collection meets every threshold in both
timing sets and in the memory and counted sets, and matches the prototype
within run-to-run variation. Bulk allocation, accepted as a cost of this
design, is unchanged from the prototype at about 170 bytes per changed item; in
the traced runs it cost at most one minor collection of about 0.4 ms per
3.4 MB of bulk updates, with no major collections, while single-item updates
triggered fewer collections than the shared map's. These are measurements of
the shipping implementation in headless Chromium on one host, at up to 1,000
items; they do not establish behavior at larger sizes or in other hosts.
