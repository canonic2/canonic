# JavaScript supported-workload roadmap

**Status:** agreed direction and sequence, requested 2026-10-25. API shapes and
lab thresholds remain proposals until reviewed. This page records intended
work; contracts and architecture distinguish supported behavior from planned
implementation. This roadmap applies to JavaScript; the
[shared framework direction](../framework.md) owns the broader platform scope.
The owner has adopted bulk membership and incremental aggregates for product
integration. Their research remains closed; original unmet quiet-host/tail gates
are preserved as evidence limitations, not rewritten as passing. No additional
research cycle is scheduled. Work stays inside Lattice.

## 1. Efficient bulk membership

Implemented and supported: `updateMembership` validates one atomic request,
constructs one frozen next order and preserves journal/rollback/incarnation
semantics. Small planner allocations and redundant abort cleanup are corrected.
The [contract](contracts/collections.md#atomic-bulk-membership) owns behavior;
the [research record](../../research/collection.md) preserves source-scoped
correctness, allocations, ownership and bounded under-load timing evidence.

## 2. Incremental aggregates

Implemented and supported: `watchAggregate` maintains explicit reversible
reductions from committed before/after records, without a per-item contribution
cache. Indexed initialization/recovery follows current keys. The
[contract](contracts/incremental-aggregates.md) owns batching, prefix delivery,
synchronous callbacks, recovery and disposal. The [research record](../../research/incremental-aggregates.md)
retains measured costs and limits. Arbitrary scanning selections remain available.

## 3. Indexed store delivery

Run an implementation lab comparing declared-field indexes with the current
subscription walk. Preserve registration order, multi-field deduplication,
reentrant commits, subscription mutation, aborts and error behavior. Include
sparse and broad updates, small and large subscriber counts, setup, teardown
and memory. Adopt only if evidence justifies the machinery. No automatic
strategy switching is authorized; inconclusive results keep the simpler walk.
The lab plan and thresholds remain to prepare and review.

## 4. Retention and fixed-height windowing

Continue the [existing investigation](../../research/large-collections.md) from
the corrected runnable task example. Its revised implementation is unmeasured;
earlier timings do not establish its performance. Finish reusable keyed
retention/reconciliation mechanics and optional fixed-height windowing across
the [composition boundary](framework/composition.md).

Keep markup, styling and presentation in Web Components. Keep application
selection policy and capability actions outside mechanics. Own row lifetimes,
incarnation replacement, cleanup and reconciliation. Resolve the proposed
helper's unchanged-range regression and assess source footprint while
preserving focus, drafts, keyboard visibility and scroll anchoring.

Retain the 4 ms scrolling and 100 ms mount targets. Calibrate measurement
before another large run, distinguishing handler time from presentation.
Recycling and variable-height rows are separate follow-ups. API adoption and
the next lab plan retain their review gates.

## 5. Normal TypeScript consumption

Direct implementation, with no performance lab: generate declarations from
existing JSDoc during package preparation, expose them through every public
entry point, and include them in the packed artifact. Author only modern
JavaScript; add no handwritten declarations, transpiled runtime or runtime
dependencies.

The [delivery contract](contracts/delivery.md#agreed-direction-generated-declarations)
owns acceptance: deterministic declaration freshness and isolated TypeScript
consumers using normal Node/browser configurations without JavaScript-source
inspection settings. Cover every entry point, generic inference and invalid
calls. This direction is agreed; generation is not implemented yet.

## Shared gates

Read contracts, code, tests and research before each design. Record comparisons,
workloads, measurement boundaries and proposed thresholds before lab runs;
obtain owner agreement before running and an adoption decision before shipping
a candidate. Agreed behavior and proposed APIs remain visibly separate.

Following acceptance, update contracts, architecture, runnable examples and
public guidance together. Repeat decisive measurements through shipping entry
points. Run checks only when explicitly requested under the workspace policy;
an approved lab authorizes its recorded comparisons, not unrelated suites.
Preserve other sessions' edits. Before any commit inspect the complete staged
index; leave nothing newly staged while awaiting a decision.
