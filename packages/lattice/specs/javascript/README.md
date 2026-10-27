# Lattice JavaScript specifications

These specifications describe Lattice's JavaScript implementation. The
[shared framework direction](../framework.md) owns the multi-language scope;
the contracts and authoring model here apply to JavaScript. Read only the area
needed for the task. Source paths in these pages are relative to the package
root unless a link states otherwise.

The [supported-workload roadmap](roadmap.md) records the agreed next stages,
their order, acceptance boundaries and outstanding review gates. It is separate
from current implementation descriptions and public API promises.

| Area                                   | Question                                                       | Contents                                                                                                  |
| -------------------------------------- | -------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| [Framework](framework/README.md)       | What does application code look like, and which approach fits? | State and presentation choices; elements, capabilities, bindings, composition and collection presentation |
| [Architecture](architecture/README.md) | How is Lattice organized internally?                           | Current store, collection, element-helper, testing and package implementation                             |
| [Contracts](contracts/README.md)       | What must an implementation guarantee?                         | Detailed behavior, edge cases, performance, delivery and acceptance evidence                              |

Contracts include supported atomic membership and
[incremental aggregates](contracts/incremental-aggregates.md), separate from
explicitly labelled proposals for other capabilities.
The store core, the keyed collection, the element helpers
and the record and collection selection declaration checks are implemented.
Capabilities, bindings and list presentation are application code using these
primitives. Lattice exports no binding, rendering, scheduling or retained-list
helper. The framework folder describes authoring conventions and explicitly
labels the retained-mechanics proposal; architecture separates current code
from explicitly labelled implementation proposals.

The public API uses `update` for record patches, `fields`/`read`/`equals` for
selections, and `(value, previous)` watcher callbacks. Membership watchers add
`change` third. [State management](contracts/state-management.md) and
[collections](contracts/collections.md) own those guarantees; the
[developer guide](../../docs/README.md) teaches the flow and the
[API overview](../../docs/api/README.md) collects the common signatures.

Implementation descriptions and test mappings are based on source, not a
record of executed checks. Acceptance tables state requirements and identify
existing coverage or gaps. Measurements and their conditions belong in
[research](../../research/README.md); they apply to the versions and workloads
those records identify, not automatically to later API or example changes.

Each detailed requirement has one owning contract. The
[package README](../../README.md) owns setup and commands.
