# Lattice research

Research records for questions settled by labs, each with its plan, results and
decision.

| Record | Status | Related contracts |
| --- | --- | --- |
| [Collection cost of bindings](collection.md) | Reported 2026-10-25: binding setup small per item; single-item updates linear in the collection size, state shape open; stage 2 adopted bulk compaction, fixing quadratic teardown; stage 3 mixed: one store per item makes updates independent of size but exceeds the memory and teardown thresholds; its follow-up found bulk updates of a derived-from field quadratic and the extra memory to be the stores themselves; stage 4 found a keyed collection prototype meeting every agreed threshold | [Keyed collections proposal](../specs/proposals/keyed-collections.md), [State management](../specs/contracts/state-management.md#collections-open), [Bindings](../specs/contracts/bindings.md), [DOM updates](../specs/contracts/dom-updates.md), [performance and evidence](../specs/contracts/performance.md) |
| [Full flow against hand-written wiring](full-flow.md) | Decided 2026-10-24: design kept; no measurable interaction overhead, measurable setup, teardown and memory costs. Stage 2 cut a changing write's allocations by about 70%, confirmed on the shipping path | [State management](../specs/contracts/state-management.md), [bindings](../specs/contracts/bindings.md), [performance and evidence](../specs/contracts/performance.md) |
| [Component authoring shape](component-authoring.md) | Decided 2026-10-24: native classes with narrow helpers | [Element lifecycle](../specs/contracts/element-lifecycle.md), [performance and evidence](../specs/contracts/performance.md) |
