# Choosing an approach

## Purpose and status

Lattice supports several ways to own state and present it. Choose against the
application's workload and required guarantees: item count alone does not say
how often items change, how long they live or whether updates must commit
together. No approach is cheapest for every workload.

**Status:** agreed guidance. Stores, keyed collections and element-local state
use existing APIs. S2's list-level selection is accepted for selective updates
and lower overhead. Fixed-height windowing is an application pattern under
evaluation; its complete responsiveness budgets remain unmet. There is no
public windowing or recycling helper. The
[performance contract](../contracts/performance.md#guidance-for-choosing-approaches)
owns how recommendations are supported and maintained.

## State ownership

| Approach | When it fits | Cost or limitation | Support |
| --- | --- | --- | --- |
| Element-local state | Focus, hover, an open menu and transient interaction owned by one element | Application facts shared by consumers need a capability owner; do not mirror them as independently writable local state | Platform authoring pattern; see [elements](elements.md) |
| Ordinary store | Related capability fields that need immutable snapshots and synchronous batches | A commit walks subscriptions and copies the snapshot on a changing write; unchanged writes stay silent | Implemented: `createStore`; see [state management](../contracts/state-management.md) |
| Items in one store field | Small collections or workflows whose consumers need a whole-state snapshot | Updating one item replaces the shared field; immutable updates can copy the whole map and consider every subscription to it | Existing store API; measured baseline, not a default for large independently changing lists |
| Keyed collection | Items with independent updates, ordered membership and bulk actions that publish together | Membership changes copy the keys array; an aggregate may scan all items; bulk writes have an accepted allocation cost | Implemented: `createCollection`; see [collections](../contracts/collections.md) |
| One store per item | Items that are independently owned capabilities, with separate state and lifetimes | Per-store memory and teardown costs; no cross-store transaction. Bulk notifications can expose intermediate state and repeat aggregate work | Existing store API; not the default collection shape |

The [collection research](../../research/collection.md) compares shared-map,
per-item-store and keyed-collection shapes, including single-item and bulk
updates, setup, memory and teardown. Its measured scope and unfavorable results
remain part of the recommendation. Independent item ownership can justify a
per-item store even though that shape missed the collection lab's limits; it
does not make those measured costs disappear.

A capability can compose scalar state and a keyed collection, for example a
filter store beside its items. They commit independently. Choose a different
boundary if consumers require an atomic change spanning both; documentation
must not imply that nesting their batches creates a cross-owner transaction.
The [capability guide](capabilities.md) shows the ordinary-store case, not a
requirement that every capability create exactly one store.

## List presentation

State organization and presentation are independent choices. A keyed collection
can feed a fully mounted list or a windowed list. The element owns row nodes,
viewport geometry and interaction; bindings supply inputs and route intent.

| Approach | When it fits | Cost or limitation | Support |
| --- | --- | --- | --- |
| Fully mounted keyed rows | The complete list fits its mount, memory and interaction budgets | Every row retains DOM and binding resources; a cheap selection handler alone does not establish fast presentation | Application pattern using existing APIs and the [DOM update contract](../contracts/dom-updates.md) |
| Windowed rows, created and destroyed | Large lists where mounted DOM dominates; the evaluated scope is fixed-height rows | Reconciliation, binding setup/teardown, focus pinning and scroll anchoring add work | Evaluated application pattern; complete mount and scrolling budgets remain unmet |
| Windowed rows with recycling | A candidate when measured construction savings outweigh rebinding and reset costs | Must prevent state leaking between items; focused rows cannot be rebound. Typical scrolling savings did not establish better tails in the lab | Experimental alternative; not the recommended default and not a public API |

Choose windowing because measured mount, memory or interaction cost requires
it, not simply because an item count crosses an undocumented cutoff. A smaller
mounted DOM is useful evidence, but does not establish the mount-presentation
or scrolling budgets. Variable-height windowing is outside the evaluated scope.

## Selection and reconciliation

For a list with one selected item, S2 keeps `selectedKey` in its capability's
store. One binding assigns that value to a public input on the list element.
The setter uses retained references to update only the old and new rows; it
does not scan every row. An unmounted selected item remains selected in the
capability and receives its selected presentation when mounted.

This has one application owner of selection. The element's input and retained
references support presentation; they are not a second source of selection
decisions. Setting the input emits no intent event. Item `selected` fields are
an alternative model only when they are the sole application owner, not a
second independently mutable copy of a selected key.

The [large-collections research](../../research/large-collections.md) supports
S2's selective CPU work and footprint. It does not establish that mounting all
rows meets the presentation budget. The accepted
[completed-reconciliation guard](collections.md#guard-completed-reconciliation)
avoids duplicate range work; its adoption does not establish that the entire
windowing candidate meets every budget or resolves its end-jump regression.

## Evidence and implementation gaps

Recommendations compare the complete lifetime, including initial setup,
changing and unchanged updates, bulk actions, allocations, memory, teardown and
presented latency where relevant. Keep resource ownership, rollback, focus,
input and key-incarnation guarantees equivalent across candidates.

The [filters example](../../examples/filters/) supplies runnable capability,
element, binding and composition code for the ordinary-store approach. The
alternative collection and list patterns above do not yet all have runnable
examples outside the lab. Such examples must exercise the shipping public
APIs and document their supported scope before being presented as complete
authoring recipes. Ignored lab prototypes are evidence, not shipped examples.

Remaining work includes those runnable examples and the unresolved windowing
presentation, scrolling and end-jump findings. Supporting multiple approaches
does not add a public helper, runtime strategy switch or automatic tuning API.
A proposed helper needs a separate contract decision and comparison against
the existing application pattern.
