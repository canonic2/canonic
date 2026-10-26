# Lattice research

Research records for questions settled by labs, each with its plan, results and
decision.

| Record | Status | Related contracts |
| --- | --- | --- |
| [Collection cost of bindings](collection.md) | Decided 2026-10-26: keyed collection accepted in stages 4/5; bulk membership rejected for this version after bounded final acceptance. Small-append allocation fits and measured large actions improve, but setup/retention gates and complete churn/confirmation are not established. Experimental runtime discarded; independent cleanup fixes retained. Earlier evidence and revisions preserved. | [Collections](../specs/contracts/collections.md), [State management](../specs/contracts/state-management.md#collections), [Bindings](../specs/contracts/bindings.md), [DOM updates](../specs/contracts/dom-updates.md), [performance and evidence](../specs/contracts/performance.md) |
| [Selection and windowing at 10,000 items](large-collections.md) | Reported 2026-10-25: tracing remains perturbative; runnable list example complete; mechanics preserve checked behavior but miss the no-regression rule on unchanged ranges at 1,000 items; guard accepted, large-list readiness and API adoption open | [Collections](../specs/contracts/collections.md), [Bindings](../specs/contracts/bindings.md), [DOM updates](../specs/contracts/dom-updates.md), [performance and evidence](../specs/contracts/performance.md) |
| [Full flow against hand-written wiring](full-flow.md) | Decided 2026-10-24: design kept; no measurable interaction overhead, measurable setup, teardown and memory costs. Stage 2 cut a changing write's allocations by about 70%, confirmed on the shipping path | [State management](../specs/contracts/state-management.md), [bindings](../specs/contracts/bindings.md), [performance and evidence](../specs/contracts/performance.md) |
| [Component authoring shape](component-authoring.md) | Decided 2026-10-24: native classes with narrow helpers | [Element lifecycle](../specs/contracts/element-lifecycle.md), [performance and evidence](../specs/contracts/performance.md) |
