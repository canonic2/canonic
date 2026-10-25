# Choosing an approach

Choose state ownership first, then choose how much of the interface to mount.
Consider which values change together, how often items change, how long they
live and whether consumers need a complete snapshot.

## Choose a state model

| Your requirement | Start with | What to watch |
| --- | --- | --- |
| One element owns a temporary interaction | [Element-local state](local-state.md) | Do not duplicate application decisions inside the element |
| Related scalar fields change as one action | [Ordinary store](stores.md) | Snapshots are shallowly frozen; replace nested data |
| A small item set belongs in one whole-state snapshot | [Items in one field](shared-state-collections.md) | Copying the item map and evaluating its consumers grows with its size |
| Each item is an independent capability | [Independent item stores](independent-items.md) | A store per item adds memory and has no cross-store transaction |
| Many items change independently, including bulk actions | [Keyed collection](keyed-collections.md) | Membership changes copy the keys; aggregates can still scan all items |

Use the simplest model that meets the required behavior and your application's
performance budget. A specialized collection is useful when it removes work
you actually perform; it is not necessary for a handful of settings.

## Choose list presentation

| Your requirement | Start with | What to watch |
| --- | --- | --- |
| Mounting the complete list is affordable | [Fully mounted keyed rows](mounted-lists.md) | Total DOM, subscriptions, initial layout and teardown |
| Too many mounted rows dominate memory or rendering | [Windowing](windowing.md) | Range reconciliation, focus, scrolling and row lifetimes |
| Creating entering rows is a measured bottleneck | Compare [row recycling](row-recycling.md) | Resetting item state, rebinding costs and slow frames |

There is no universal item-count cutoff. A hundred complex editors and a
thousand simple labels can have very different costs. Measure the actual row,
including its controls and event listeners.

## Keep selection separate from row subscriptions

For a single selected item, keep a selected key in one capability and assign
it to one list input. Let the list update only the old and new retained rows.
See [List selection](list-selection.md). Having every row watch one shared
selected key performs a check per row even when only two rows change.

## Compare the complete lifetime

Check setup, isolated updates, unchanged updates, bulk actions, allocation,
memory, teardown and visible response. Include keyboard focus, text entry,
removal and reordering in correctness checks. Improving a typical handler
does not establish that slow frames or first presentation improved.

Stores, keyed collections and element helpers are available APIs. Windowing
and recycling are application techniques, with no Lattice helper to import.
The evaluated fixed-height windowing techniques do not yet meet every
responsiveness target; recycling is experimental rather than a default.
