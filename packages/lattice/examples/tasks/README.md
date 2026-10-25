# Task collection example

A runnable list built with shipping `createCollection`, `createStore` and native
Web Components. It contains application code, not a framework windowing helper.

From the repository root, run:

```sh
node packages/lattice/examples/serve.js
```

Open [the task list](http://127.0.0.1:4177/examples/tasks/) for 10,000 items,
or add `?count=1000` for 1,000 items. Stop the local server with Ctrl+C.
The same server can open the filters example at `/examples/filters/`.

The capability is created independently of the view. `tasks.js` owns keys,
completion actions, bulk commits and the single selected key. `bindings.js`
assigns row inputs and connects intent to actions. `task-list.js` owns fixed
32 px geometry, keyed DOM placement and row lifetimes. `task-row.js` owns
markup, styling and native controls. The list's selectedKey input patches only
the old and new retained rows.

Only visible rows plus four rows of overscan on each side are mounted. A
focused row remains mounted as one additional row when it leaves the range;
its draft input survives. Arrow keys mount the destination before focusing it.
Removing a focused item focuses the next item at its old index, the previous
item at the end, or the list when empty. Insertion/removal above the viewport
preserves its anchor. Replacing a key ends its bindings and creates a new row,
including replacement inside one batch. Rows are not recycled.

The completed-range guard includes membership revision and focused key.
Membership delivery invalidates it even when key strings are unchanged.
Changing the viewport height invalidates geometry. Bindings continue to update
item inputs without range reconciliation.

Tear down the view by aborting its signal before removing its owner. Capability
state may outlive the view. Detached rows have no active item bindings.
This example does not establish the 4 ms scrolling or 100 ms presentation
targets, general large-list readiness, variable-height rows or recycling.

The repeated row-lifetime and reconciliation work is a candidate for reusable
framework mechanics, with explicit callbacks for application presentation and
binding. The [research record](../../research/large-collections.md) records
measurement limits and the comparison process.
