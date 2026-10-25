# Windowing fixed-height lists

Windowing mounts the rows in view plus a small overscan instead of keeping
the entire list's DOM alive. Use it when measured mount, memory or rendering
costs make [fully mounted rows](mounted-lists.md) too expensive.

This is an application technique using existing Lattice APIs. Lattice exports
no windowing helper. The evaluated fixed-height techniques have not met every
mount-presentation and scrolling target, so measure your row and host before
relying on a responsiveness budget. Variable-height rows require a different
measurement and positioning strategy.

## Calculate a retained range

Use a scrollable extent for the complete result set and position retained rows
by their index. The row stride must match its actual height and any spacing;
the calculation below assumes uniform rows with no additional gaps. With a
known positive row height, the range is inexpensive:

```js
/**
 * Calculates an application's fixed-height retained range, end exclusive.
 * @param {number} count Number of rows, a nonnegative integer.
 * @param {number} scrollTop Scroll offset in CSS pixels.
 * @param {number} viewportHeight Visible height in CSS pixels.
 * @param {number} rowHeight Known positive row height in CSS pixels.
 * @param {number} overscan Nonnegative number of extra rows per side.
 * @returns {{ start: number, end: number }} Indices to retain.
 */
function retainedRange(count, scrollTop, viewportHeight, rowHeight, overscan) {
  const start = Math.max(0, Math.min(count, Math.floor(scrollTop / rowHeight) - overscan));
  const end = Math.max(start, Math.min(count, Math.ceil((scrollTop + viewportHeight) / rowHeight) + overscan));
  return { start, end };
}
```

Read the required geometry before writing positions. Resolve entering and
leaving keys, retain overlapping rows, and create or release only rows that
cross the range boundary. A removed item's incarnation must be released even
if a replacement uses the same string key.

Only mounted items need row bindings. Updates to an unmounted item change its
collection record; its current values are applied when the row mounts. Bulk
actions can still change all data items and evaluate an aggregate once, while
assigning inputs only to mounted rows.

## Avoid reconciling the same range twice

If both a frame callback and a native scroll event request reconciliation,
compare against the last successfully completed range before constructing
desired-key arrays or doing DOM work. Include membership identity, range,
focused-row identity and geometry inputs that affect placement.

Key-array equality alone is insufficient: a remove-and-reinsert action can
keep the same string keys while replacing an incarnation. Apply removal
cleanup and invalidate the range on those membership notifications. Save the
completed range only after reconciliation succeeds.

Coalesce scroll presentation work if needed, while keeping application state
acceptance synchronous. Cancel pending frame work when its owner ends. There
is no need to put every scroll pixel in an application store.

## Preserve focus, drafts and scroll position

- Keep a focused row mounted when it leaves the window, bounded to one
  additional pinned row. Its normal bindings remain active.
- Mount a keyboard destination before focusing it. Expose the full list's
  logical position and size using the appropriate accessible semantics for
  your widget, rather than letting the small mounted subset redefine it.
- When removing a focused item, apply a documented fallback: the next item,
  the preceding item at the end, or the list when empty. Handle replacement
  of the focused incarnation even if its key is reused.
- Preserve the first visible key and its pixel offset when inserting or
  removing above the viewport. Define a fallback when that anchor is removed.
- A focused offscreen row remains mounted, so an update to that row can
  legitimately write DOM. The zero-write goal applies to unmounted items.

## Measure the additional work

Windowing reduces DOM, but adds reconciliation and entering/leaving binding
lifetimes. Check first presentation, ordinary scrolling, full-screen steps,
end jumps, edits, membership changes, allocation, garbage collection and
teardown. Track slow frames as well as typical handler cost. An animation-frame
callback is a paint opportunity, not proof that a frame was presented.

Start by creating and destroying entering/leaving rows. Compare
[recycling](row-recycling.md) only if row construction is a measured cost
worth the extra reset and lifecycle work.
