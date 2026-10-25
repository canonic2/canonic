# Ordinary stores

Use `createStore` for related application fields: a query and its lock state,
a dialog's workflow, or a requested destination and loading phase. One action
can change several fields in one patch or synchronous batch.

## Read and change state

```js
import { createStore } from '@canonic2/lattice';

const store = createStore({ query: '', locked: false });
const before = store.get();

store.set({ query: 'Acme' });
store.set((state) => ({ locked: !state.locked }));

store.batch(() => {
  store.set({ query: '' });
  store.set({ locked: false });
});
console.log(before.query); // Still '', regardless of subsequent writes.
```

`get()` returns a frozen snapshot. The snapshot never changes after it is
returned; unchanged writes preserve its identity and notify nobody. Freezing
is shallow: replace nested objects and arrays rather than mutating them.
An unknown field throws instead of adding another state field.

A batch defers notification, not writes: reads inside it see pending changes.
If its callback throws, its changes roll back. Nested batches are savepoints.
Callbacks must be synchronous; returning a promise or thenable rolls back and
throws a `TypeError`. Work after an `await` belongs in a separate commit.

## Subscribe to the value you need

```js
/** @typedef {{ query: string, locked: boolean }} FilterState */

/** @satisfies {import('@canonic2/lattice').Selection<FilterState, string>} */
const query = { fields: ['query'], read: (state) => state.query };
const lifetime = new AbortController();

store.watch(query, (value, previous) => {
  console.log({ value, previous });
}, { signal: lifetime.signal });

store.set({ locked: true }); // Does not call query.read.
store.set({ query: 'Acme' });
lifetime.abort(); // Ends this subscription.
```

The callback runs immediately with `previous` set to `undefined`, then when
one of the declared fields changes and the selected value differs. Equality
defaults to `Object.is`. Supply `equals: shallow`, imported from the root
entry point, for a selection that creates a plain object or array and needs
comparison of its immediate contents.

Readers must be pure and declare every field they depend on. The
test-only `checkSelection` export can check representative snapshots; it does
not run in production. The [filters tests](../examples/filters/filters.test.js)
show this alongside real capability selections.

## Put actions behind a capability

Keep `set` and `batch` private. Expose `get` and `watch` as a read-only state
view, selections beside their readers, and named actions such as `setQuery`.
Inject persistence and network operations into the capability instead of
importing them into an element. The
[filters capability](../examples/filters/filters.js) shows this shape.

Bindings assign selected values to public element inputs. Give them the
lifetime they serve; if setup fails partway through, release any subscriptions
already acquired. The [filters bindings](../examples/filters/bindings.js) show
setup rollback and synchronous and asynchronous action-failure handling.

## Understand delivery and errors

Commits deliver synchronously. A write made by a subscriber becomes current
immediately but its notifications queue behind the delivery in progress. The
outermost write drains those notifications before returning.

Delivery errors go to the store's `report` option; they do not undo a commit
or throw out of the committing write. Its default uses platform `reportError`
where available, otherwise an uncaught microtask error. An initial `watch`
failure propagates and leaves no subscription. A subscription whose callback
failed stays subscribed and retains its last successfully delivered value.

## When to use another approach

Changing writes copy the state snapshot, and delivery walks subscriptions to
find interested readers. That is a reasonable starting point for small related
state. For a field containing many independently updated items, consider a
[keyed collection](keyed-collections.md). For interaction private to one
element, use [local state](local-state.md).
