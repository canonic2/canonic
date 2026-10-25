# Collection presentation patterns

Application elements own row presentation and window geometry. The keyed
[collection](../contracts/collections.md) supplies membership and item inputs;
it does not mount rows or choose a viewport.

## Ownership

| Area | Owner |
| --- | --- |
| Ordinary, shared-map and per-item stores | `createStore`; application conventions compose them |
| Keyed records, incarnations and bulk commits | `createCollection` |
| Which item is selected | Application capability policy; a list input patches affected retained rows |
| Keyed row reconciliation | Candidate reusable mechanics for retained identity, incarnation replacement and cleanup |
| Windowing | Candidate optional mechanics for retained ranges, entering/leaving lifetimes and focus pinning |
| Recycling | Experimental until benefits justify reset and lifecycle complexity |

Framework collection mechanics coordinate retained items and their lifetimes
through explicit callbacks. Elements supply presentation and placement;
application bindings connect inputs and intent to capability actions. Mechanics
do not choose markup, styling or selection policy. Each state-model convention
does not require a separate framework API.

The runnable [task collection example](../../examples/tasks/README.md) uses
shipping APIs and the guard below. Its hand-written coordination is a baseline
for identifying the smallest reusable mechanics, not a requirement that every
application maintain its own reconciliation and windowing implementation.
No shared presentation API is adopted by the example itself.

## Guard completed reconciliation

When both an animation-frame callback and a native scroll event can render a
fixed-height window, they can request the same work. The list element keeps
the identity of its last completed reconciliation:

- the membership revision;
- the first and last indices of the retained range;
- the focused item's key, including a retained row outside that range.

Read the current range and focus before changing the DOM. If all four values
match the completed reconciliation, return before constructing the desired
keys, position map or reconciliation data. Save the identity only after the
reconciliation and position writes complete successfully.

Advance the membership revision on every membership callback. Comparing only
the keys array or its string keys is insufficient: removing and reinserting
a key creates a new incarnation even when the final key order is identical.
Apply that removal and its binding cleanup before considering the guard.

Focus changes also invalidate the completed range. Keep the focused row mounted
when it leaves the window, bounded to one extra row; never recycle that element
onto another item. Mount a keyboard destination before focusing it. When an
item is removed, apply the element's documented deterministic focus fallback.
Insertion or removal above the viewport must preserve its scroll anchor.

The guard belongs to the list element. Item bindings continue to assign their
inputs, and a list-level selection input continues to update the affected
retained rows independently of range reconciliation. Geometry, row-height or
other presentation inputs that affect positions must invalidate the guard too.

This application pattern is accepted within the fixed-height scope measured
by the [large-collections research](../../research/large-collections.md).
It removes redundant work; it does not establish general large-list readiness.
The mount presentation and scrolling targets remain unmet. The combined
candidate's end-jump regression remains under investigation. Lattice supplies
no windowing helper, and this pattern does not authorize a new API.
