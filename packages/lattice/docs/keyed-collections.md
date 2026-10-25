# Keyed collections

Use `createCollection` for ordered items that change independently: tasks,
records or table rows. A single-item update replaces one frozen record;
one bulk action can publish all its changes in a single commit.

## Create and update items

```js
import { createCollection } from '@canonic2/lattice/collection';

const tasks = createCollection([
  ['t1', { label: 'Write the plan', done: false }],
  ['t2', { label: 'Review', done: false }],
]);

tasks.update('t1', { label: 'Write the guide' });
tasks.update('t2', (task) => ({ done: !task.done }));
tasks.insert('t3', { label: 'Publish', done: false }, 't2');
tasks.reorder(['t2', 't1', 't3']);
tasks.remove('t3');
```

Keys are unique strings. `insert` appends unless you pass a key to insert
before. `reorder` takes a permutation of the current keys. Updating or
removing a missing key throws; `get(key)` returns `undefined` when absent.
Each item's fields are established when it is inserted; an unknown update
field throws.

`keys()` returns a frozen array in order, and `get(key)` a frozen item record.
Keep their references: each stays unchanged after being returned. Freezing is
shallow, so replace nested data. Item updates do not replace unrelated records
or the keys array. There is no whole-collection snapshot; create one explicitly
when persistence or another consumer needs it.

## Watch an item or an aggregate

```js
/** @typedef {{ label: string, done: boolean }} Task */
/** @satisfies {import('@canonic2/lattice/collection').ItemSelection<Task, string>} */
const label = { fields: ['label'], read: (task) => task.label };

/** @satisfies {import('@canonic2/lattice/collection').CollectionSelection<Task, number>} */
const openCount = {
  items: ['done'],
  read: (view) => {
    const keys = view.keys();
    let count = 0;
    for (let index = 0; index < keys.length; index++) {
      const key = /** @type {string} */ (keys[index]);
      if (view.get(key)?.done === false) count++;
    }
    return count;
  },
};
const lifetime = new AbortController();

tasks.watchItem('t1', label, (text) => console.log(text), { signal: lifetime.signal });
tasks.watch(openCount, (count) => console.log(count), { signal: lifetime.signal });
```

Watchers run immediately, then when their dependencies and selected value
change. An item watcher only considers updates to that item and its declared
fields. Aggregate `items` names the fields read across items; membership and
reorder changes also cause evaluation. A label update does not recount the
open items above. An aggregate can still scan every item when evaluated.

Use `watchKeys` for membership and order. Its callback receives the keys and
an optional change with `inserted` and `removed` keys; the initial call has no
change. Removing and reinserting the same key reports both changes, so the
list can replace the old row incarnation.

## Batch a bulk action

```js
tasks.batch(() => {
  const keys = tasks.keys();
  for (let index = 0; index < keys.length; index++) {
    const key = /** @type {string} */ (keys[index]);
    tasks.update(key, { done: true });
  }
});
lifetime.abort();
```

The batch is synchronous. Reads inside it see pending writes; subscribers
see its committed result. A matching aggregate runs at most once; its callback
is skipped if its selected result is unchanged. A thrown callback rolls back;
nested batches are savepoints, and returning a promise or thenable throws.
Register watchers outside batches.

Delivery runs membership watchers first, changed-item watchers second and
aggregate watchers last. Writes from subscribers queue notifications behind
the current delivery; readers receive the latest state when notified.
Subscriber failures use the collection's `report` option, as with a store.

## Ownership and removal

Keep the writable collection private to a capability. Expose read methods and
watchers plus named actions. If you expose `size` through a wrapper, forward
it with a getter; copying its current number would become stale.

Committed removal ends that item's watchers before membership delivery. A
batch rollback restores the item and its watchers unless their own signal
was aborted. Reinserting a key creates a new incarnation with no old watchers.
End the removed row's own lifetime too, to release its event listeners and
other resources. See [Fully mounted lists](mounted-lists.md).

Collections avoid whole-map copying for item edits, but insertion/removal
copy the keys array and bulk edits allocate new item records. A scalar store
beside a collection commits independently; there is no transaction spanning
both. Use [items in one store field](shared-state-collections.md) if a
whole-state snapshot is the requirement that matters most.
