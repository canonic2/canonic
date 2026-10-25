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
assigns row inputs and connects intent to actions. `list-view.js` owns keyed
reconciliation and row-binding lifetimes outside presentation. `task-list.js`
receives immutable `keys` and `items` projections and owns fixed 32 px geometry,
keyed DOM placement, anchoring and selection decoration. `task-row.js` owns
markup, styling and native controls. The list's selectedKey input patches only
the old and new retained rows.

Only visible rows plus four rows of overscan on each side are mounted. A
focused row remains mounted as one additional row when it leaves the range;
its draft input survives. Arrow keys mount the destination before focusing it.

The list emits range requests for observed scrolling/focus departure and a key
for observed keyboard navigation. Composition uses the public geometry and
focus methods, mounts the destination, then focuses it. `view.focusKey(key)`
does the same for an application command. `view.render()` reconciles after a
programmatic scroll or mode change. Setters and connection callbacks emit no
intent, acquire no application subscriptions and call no application callbacks.

Removing a focused item focuses the next item at its old index, the previous
item at the end, or the list when empty. Insertion/removal above the viewport
preserves its anchor. Replacing a key ends its bindings and creates a new row,
including replacement inside one batch. Rows are not recycled.

Composition's completed-range guard includes membership revision and focused key.
Membership delivery invalidates it even when key strings are unchanged.
Changing the viewport height invalidates geometry. Bindings continue to update
item inputs without range reconciliation.

Temporary disconnection releases host listeners and the list's geometry observer;
it retains inputs, nodes and drafts. Atomic `moveBefore` moves preserve the
connection and focus. Row bindings and composition's observer belong to the
explicit view signal, remain owned across a temporary detach, and end when a
row leaves retention, its incarnation ends or the view is disposed. Composition
does not acquire entering rows while the list is detached. Abort the view signal
for final teardown; it ends row bindings before removing presentation. Capability
state may outlive the view. A released row has no active item bindings.

Both elements capture declared inputs assigned before upgrade. Constructor-safe
inputs apply immediately. Inputs that place supplied rows or write host selection
attributes replay after upgrade; newer assignments supersede the captured value.
Connection setup rolls back its listeners and observer if rendering fails.
This example does not establish the 4 ms scrolling or 100 ms presentation
targets, general large-list readiness, variable-height rows or recycling.

The repeated row-lifetime and reconciliation work is a candidate for reusable
framework mechanics, with explicit callbacks for application presentation and
binding. The [research record](../../research/large-collections.md) records
measurement limits and the comparison process.
