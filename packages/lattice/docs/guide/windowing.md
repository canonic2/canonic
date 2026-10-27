# Large lists

A list can have cheap item updates and still be expensive to mount. Every row
retains DOM, listeners and subscriptions, and the browser must lay it out.

Windowing limits mounted rows to the visible range plus a small margin.
It is an application technique using Lattice's existing APIs; there is no
windowing helper to import.

## Start with a working list

Begin with a [fully mounted list](list-rendering.md). Measure mounting, retained
memory and interaction with your actual row before adding windowing.

A simple label and a rich editor can have very different costs at the same
item count. There is no universal cutoff.

The [runnable task example](../../examples/tasks/README.md) uses fixed-height
rows. It demonstrates the ownership and interaction behavior discussed below.
Its mount-presentation and scrolling budgets are not established as met; treat
those as measurements to make in your host.

## Calculate the visible range

For fixed-height rows, derive the range from scroll position and viewport
height:

```js
function visibleRange(count, scrollTop, viewportHeight, rowHeight, overscan = 4) {
  const first = Math.floor(scrollTop / rowHeight);
  const last = Math.ceil((scrollTop + viewportHeight) / rowHeight);

  return {
    start: Math.max(0, Math.min(count, first - overscan)),
    end: Math.max(0, Math.min(count, last + overscan)),
  };
}

visibleRange(1000, 320, 480, 32);
// { start: 6, end: 29 }
```

The end index is exclusive. This calculation assumes a nonnegative scroll
offset, a known positive row stride and no extra gaps. Variable-height rows
need a different measurement and positioning strategy.

Use an extent representing the complete list height and position retained rows
by their full-list index. Read geometry before writing row positions.

## Give retained rows their own lifetimes

Keep overlapping rows. Create and bind entering rows; end the bindings of rows
that leave retention.

An unmounted item's record still changes. When its row is created later,
the watcher's initial callback supplies the current values.

Item removal is different from leaving the viewport. Removal ends that
incarnation even if a replacement has the same key. Apply membership removals
before reusing a completed-range result.

## Avoid duplicate reconciliation

Scroll, focus and geometry callbacks can request the same range more than once.
Remember the identity of the last successfully completed reconciliation:

- Membership revision.
- Start and end indices.
- Focused key, including a row pinned outside the range.
- Geometry inputs that affect placement.

Skip repeated work only when all relevant values match. Advance the revision
on every membership notification, including replacement with unchanged string
keys. Save the completed identity after reconciliation succeeds.

The [task view composition](../../examples/tasks/list-view.js) implements this
guard without adding a scheduler around state changes.

## Preserve the interaction

A focused row can leave the visible range while a person is editing it. Keep
that row mounted as one additional pinned row so its draft and selection
survive.

Mount a keyboard destination before focusing it. Removing the focused item
needs a deterministic fallback: the next item, the preceding item at the end,
or the list when empty.

Preserve the first visible key and its pixel offset when inserting or removing
above the viewport. Expose logical positions according to the accessibility
semantics of your widget; a small mounted subset is still part of the full list.

## Recycling is an additional decision

Recycling rebinds an unused row element to a different item. It requires ending
the old binding lifetime, resetting all item-specific presentation, and
creating a new binding lifetime. A focused row must stay with its item.

Do not add a pool just to create fewer nodes. It retains memory and can transfer
drafts, validation or pending work to the wrong item unless reset is complete.
The evaluated recycling technique remains experimental and has no Lattice API.

Start with create-and-destroy windowing. Compare recycling only when row
construction is a measured bottleneck, including slow frames and teardown.

## Measure what the person experiences

Include initial presentation, ordinary scrolling, viewport-sized steps, end
jumps, edits, membership changes and cleanup. Report allocations and retained
memory as well as handler time.

A quick callback or an animation-frame callback does not prove that an updated
frame was presented. The existing examples establish useful behavior; they do
not establish a responsiveness budget for a different row or browser host.

**Next:** [Testing](testing.md)
