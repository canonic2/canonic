# List selection without visiting every row

Keep the selected key in one application capability and bind it to one public
input on the list element. The list can update the old and new retained rows
without making every row subscribe to the shared selected key.

## Assign one list input

A binding applies the capability's selection directly:

```js
// list is your application's element; selection is your capability.
selection.state.watch(
  selection.select.selectedKey,
  (key) => { list.selectedKey = key; },
  { signal },
);
```

The element emits an intent event when a user chooses an item; an event binding
calls the capability's named selection action. Setting `selectedKey` must not
emit that event. The capability validates membership and owns the policy for
what happens when a selected item is removed.

## Patch retained rows

Inside your list class, retain a map from keys to row elements. The following
setter fragment assumes `#rows` contains your rows and each row has a public
boolean `selected` input:

```js
/** @type {string | null} */
#selectedKey = null;

/** @param {string | null} key The selected item, or no selection. */
set selectedKey(key) {
  if (Object.is(key, this.#selectedKey)) return;
  const previous = this.#selectedKey === null ? undefined : this.#rows.get(this.#selectedKey);
  const next = key === null ? undefined : this.#rows.get(key);
  this.#selectedKey = key;
  if (previous) previous.selected = false;
  if (next) next.selected = true;
}
```

This is application code, not a Lattice list widget. As with other public
inputs, handle values assigned before element upgrade. The
[filter-field example](../examples/filters/filter-field.js) shows input
capture and replay for an existing element.

When mounting a row, set its selected input from the current `#selectedKey`.
This matters for [windowing](windowing.md): an unmounted selected item stays
selected in application state and shows that state when it enters the window.
Removing and reinserting a key creates a new row incarnation.

## Keep one application owner

An alternative is to store selected flags on items and update the old and new
flags in one collection batch. Use that only if those flags are the sole
application selection owner. Do not also maintain an independently writable
selected-key field.

The list-input approach avoids that additional item state and per-row
selection subscriptions. It does not remove the cost of mounting or rendering
the list, and it does not guarantee a particular click-to-presentation time.
Keep selection and keyboard focus policies explicit: selecting an item does
not inherently require moving focus to it.
