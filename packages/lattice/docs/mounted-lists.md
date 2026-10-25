# Fully mounted keyed lists

Keep every row mounted when the complete list fits your application's setup,
memory and interaction budgets. This is the simplest presentation model:
rows and their bindings live as long as their item incarnations.

Lattice supplies the [keyed collection](keyed-collections.md). Your list
element owns nodes, row placement and interactions; it does not import a store.
Application composition and membership wiring supply its rows and bind their
public inputs.

## Give each row an identity and a lifetime

Use a key-to-row registry rather than array indices as identity. Keep that
registry with the presentation owner. Each row also has a lifetime, commonly
an `AbortController`, that ends its item subscriptions and event listeners
together. Application composition connects row creation, binding and cleanup
through the presentation owner's public interface; Lattice supplies no list
or row-factory API.

Subscribe to membership using `watchKeys`. The initial callback has no change
object: reconcile the complete initial keys. For subsequent callbacks:

1. Release rows whose incarnations were removed, including keys listed in
   both `removed` and `inserted`.
2. Create and bind rows for new incarnations. Register each row's item
   watchers with its lifetime signal.
3. Move retained nodes into the requested order. Preserve their identity;
   do not replace the list's HTML to reorder it.

Membership delivery precedes item delivery. A newly created item watcher gets
its initial value synchronously and skips that commit's later delivery.
The presentation owner creates, places and removes nodes; application wiring
connects each row's public inputs for its lifetime. A binding does not query
the list's private DOM or decide membership, and an element does not import
the collection to subscribe itself.

## Keep updates local

Row bindings subscribe to their own item fields. For example, a label watcher assigns
the row's public `label` input. The element's setter compares the input and
patches its retained text node. A label edit should not reconstruct the row,
its controls or neighboring rows.

Use a [list-level selection input](list-selection.md) for a single selected
key. Avoid a shared selection subscription on every row when only two rows
need to change.

## Remove resources as well as nodes

On removal, end the row's lifetime and remove its element through the owning
presentation interface. Removing a collection item already ends its item
watchers, but does not release the row's separate event listeners or observers.
If creating a row or binding fails, release resources acquired for that row
and propagate the failure. Disposing the complete list attempts every owned
cleanup, even if one fails.

Use native state-preserving moves such as `moveBefore` where available and
appropriate. If the host uses a move that disconnects and reconnects an element,
its connection cleanup must be safe and reconnection must not duplicate work.
Check actual focus, text selection and scroll behavior in your supported host.

## When to window

Fully mounting keeps stateful controls attached and avoids window reconciliation
on scroll, but every row retains DOM and binding resources. Initial style,
layout and paint can dominate even when item updates are selective.

If that cost exceeds your budget, compare [windowing](windowing.md) using your
real row. There is no fixed item-count threshold that makes full mounting
right or wrong for every application.
