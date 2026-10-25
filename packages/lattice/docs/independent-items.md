# Independent item stores

Give each item an ordinary store when it is a separately owned capability:
for example, independent editor sessions with their own actions and resources.
Use a [keyed collection](keyed-collections.md) when the items are primarily
rows in one collection with bulk actions.

## Share selections, keep ownership separate

```js
import { createStore } from '@canonic2/lattice';

const first = createStore({ title: 'First draft', dirty: false });
const second = createStore({ title: 'Second draft', dirty: false });

/** @satisfies {import('@canonic2/lattice').Selection<{ title: string, dirty: boolean }, string>} */
const title = { fields: ['title'], read: (state) => state.title };
const lifetime = new AbortController();

first.watch(title, (value) => console.log(value), { signal: lifetime.signal });
second.set({ title: 'Updated second draft' }); // Does not visit first's watchers.
lifetime.abort();
```

The containing capability owns membership and order, creates item
capabilities, and exposes their read-only state views. A method such as
`item(id)` is an application convention, not a Lattice function. Define what
it does for a missing item. Keep writes behind each capability's named actions.

## End each item's lifetime

When removing an item, end its bindings, cancel or dispose resources it owns,
and release the capability reference. `createStore` has no `dispose()` method:
its watchers end through their signals. Removing a map entry alone does not
abort subscriptions held by other consumers.

## Bulk actions are separate commits

Two stores do not share a transaction. Updating each store in a loop notifies
its subscribers before the next store changes. Nesting their `batch` calls
does not make the operation atomic, and a later failure cannot roll back
earlier commits in other stores.

An aggregate that recounts every item after each item's commit can do work
proportional to the square of the item count during a bulk update. The stores
also add memory and setup/teardown per item. These costs make this a poor
default for a large table whose rows share bulk actions.

Choose this model for independent ownership and lifetimes. Choose a
[keyed collection](keyed-collections.md) for item-local subscriptions and
one commit across many items in that collection.
