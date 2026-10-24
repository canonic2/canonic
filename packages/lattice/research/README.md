# Lattice research

Research records for questions settled by labs, each with its plan, results and
decision.

| Record | Status | Related contracts |
| --- | --- | --- |
| [Full flow against hand-written wiring](full-flow.md) | Decided 2026-10-24: design kept; no measurable interaction overhead, measurable setup, teardown and memory costs. Stage 2 cut a changing write's allocations by about 70% | [State management](../specs/contracts/state-management.md), [bindings](../specs/contracts/bindings.md), [performance and evidence](../specs/contracts/performance.md) |
| [Component authoring shape](component-authoring.md) | Decided 2026-10-24: native classes with narrow helpers | [Element lifecycle](../specs/contracts/element-lifecycle.md), [performance and evidence](../specs/contracts/performance.md) |
