# Recycling windowed rows

Recycling retains an unused row element in a bounded pool and rebinds it to
another item when needed. Consider it only after profiling a working
[windowed list](windowing.md) shows that constructing rows is a significant
cost.

This is an experimental application technique, with no Lattice helper to
import. In the evaluated fixed-height list, recycling reduced typical
scrolling work but had worse slow-frame behavior. It is not the recommended
default merely because it creates fewer elements.

## Rebind the whole item lifetime

Before reusing a row:

1. End the old item's binding lifetime so no callback or event can affect it.
2. Reset item-specific presentation through the row's public interface:
   editable values, selected state, validation, expanded state and any
   pending work.
3. Create a new lifetime and bind the replacement item's current inputs.
4. Position the row for its new logical index and place it in the retained
   range.

Resetting a reusable row is an application-owned interface, not a generic
Lattice method. Do not invent an assumption that assigning a key clears every
other input. Keep component-local async work tied to its item incarnation so
a late completion cannot update a replacement item.

## Protect stateful interactions

A focused row must stay pinned to its item and must not enter the pool.
Preserve its draft and text selection while it is pinned. If unmounting a
nonfocused row would lose a draft the product promises to keep, save it with
the appropriate application owner before recycling. Do not transfer that draft
to the next item assigned to the element.

Retaining an element does not mean retaining its old bindings. Rebinding still
costs subscriptions and listeners. Also account for connection resources:
removing a row can disconnect it, while placing it in a hidden connected pool
can keep its observers active. Choose and document one ownership policy.

Bound the pool, release surplus rows, and release every pooled row at list
teardown. Compare memory and cleanup as well as construction count.

## Choose from measured behavior

Compare recycling with create-and-destroy windowing on the same rows and
workloads. Include full-screen scroll steps, end jumps, focus pinning, item
removal/reinsertion, stale callbacks, retained drafts and garbage collection.
Prefer simpler windowing when typical savings do not justify tail latency,
memory or interaction risks. Do not reuse rows in a production list solely
because a different row type benchmarked well.
