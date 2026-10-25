# Lattice contracts

These topics own required behavior, edge cases, decisions and acceptance
evidence. They are agreed contracts; of the capabilities they describe, the
store core, the element helpers and the selection declaration check are
implemented.
The [framework](../framework/README.md) folder shows how authors meet them, and
the [architecture](../architecture/README.md) folder how Lattice implements them.

| Topic | Owns |
| --- | --- |
| [Delivery and public API](delivery.md) | Native ESM, zero runtime dependencies, entry points, compatibility, import isolation, tree shaking, consumer types and versions |
| [Performance and evidence](performance.md) | Work counts, allocations, rendering and latency, representative workloads, fair comparisons and regression evidence |
| [State management](state-management.md) | Snapshots, commits, batches, selections, delivery, capability conventions and asynchronous operations |
| [Bindings](bindings.md) | Assignments, events, ordering, setup failure, action reporting and lifetime |
| [Resource lifecycle](resources.md) | Shared ownership, cleanup, cancellation, partial initialization and cleanup failures |
| [Element lifecycle](element-lifecycle.md) | Construction, early inputs, connections, moves, events and registration |
| [DOM updates](dom-updates.md) | Retained nodes, writes, keyed collections, interaction preservation and layout |
| [Scheduling](scheduling.md) | Synchronous defaults, justified deferral, frame coalescing, background work and cancellation |

Each guarantee has one owner. Instructions for contributors live in the
package's agent rules and README. Verification runs only when requested;
acceptance criteria do not claim an implementation or authorize running checks.
