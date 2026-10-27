# Collections

A store is convenient for one related record. A keyed collection holds many
records that change independently, while still letting one action update them
in a single batch.

The update and selection patterns are the same ones you already know.

## Create a task collection

```js
import { createCollection } from '@canonic2/lattice/collection';

const tasks = createCollection([
  ['guide', { text: 'Write the guide', done: false }],
  ['review', { text: 'Review the examples', done: false }],
]);
```

Each entry pairs a unique string key with a record. The key identifies an item;
the record holds its data.

```js
console.log(tasks.get('guide').text); // 'Write the guide'
console.log(tasks.keys()); // ['guide', 'review']
console.log(tasks.size); // 2
```

Records and the ordered keys array are shallowly frozen. Unchanged records
keep their identities.

## Update one item

A collection adds a key before the familiar patch argument:

```js
tasks.update('guide', { done: true });
tasks.update('review', (task) => ({ done: !task.done }));
```

Only that item's record is replaced. Other records and the keys array remain
the same. Item updates do not require copying a whole item map.

As with stores, replace nested data and treat unchanged writes as silent.
An update to a missing item or unknown field throws.

## Watch an item

```js
const text = { fields: ['text'], read: (task) => task.text };
const lifetime = new AbortController();

tasks.watchItem(
  'guide',
  text,
  (value) => {
    console.log('Guide title:', value);
  },
  { signal: lifetime.signal },
);
```

`watchItem` receives a key, a selection, a callback and options. Its reader
gets one item record, and its callback gets `(value, previous)`.

You can reuse `text` for another key. A change to one item does not evaluate
another item's subscriptions.

## Derive a collection-wide value

Use `watch` for a summary across items:

```js
const openCount = {
  fields: ['done'],
  read: (view) => {
    let count = 0;
    for (const key of view.keys()) {
      if (view.get(key).done === false) count++;
    }
    return count;
  },
};

tasks.watch(
  openCount,
  (count) => {
    console.log('Open tasks:', count);
  },
  { signal: lifetime.signal },
);
```

The reader receives a traversal view with `keys()`, `get(key)`, `has(key)`
and `size`. It has no mutation or subscription methods.

`fields` names fields across items. A text edit skips `openCount`; a completion
change can evaluate it. Membership and order changes also evaluate collection
selections, because readers can depend on which items exist and their order.

For membership alone, an empty field list is useful:

```js
tasks.watch(
  {
    fields: [],
    read: (view) => view.size,
  },
  (count) => console.log('Task count:', count),
  {
    signal: lifetime.signal,
  },
);
```

The collection controls when a reader runs. A reader that counts every item
still does work proportional to the collection's size.

For a reversible count or sum that changes frequently, use
[`watchAggregate`](aggregates.md) to maintain it from changed records instead.

## Change membership and order

```js
tasks.insert('publish', { text: 'Publish the guide', done: false });
tasks.insert('outline', { text: 'Outline the guide', done: false }, 'guide');

tasks.remove('outline');
tasks.reorder(['publish', 'guide', 'review']);
```

`insert` appends unless its third argument names an existing key to insert
before. `reorder` accepts a permutation of the current keys. Missing keys,
duplicate insertion and invalid orders throw.

## Batch a bulk action

```js
tasks.batch(() => {
  for (const key of tasks.keys()) {
    tasks.update(key, { done: true });
  }
});
```

The batch publishes its completed result once. Each matching aggregate is
evaluated at most once for that commit. Nested savepoints, rollback and
synchronous callbacks work as they do for stores.

Batching combines delivery; it does not combine membership array copies.
Each `insert` or `remove` still creates a new frozen keys array. Reads inside
the batch observe every completed operation. For many membership changes,
use `updateMembership` to construct one next order instead:

```js
tasks.updateMembership({
  remove: ['review'],
  insert: [['next', { text: 'Plan the next guide', done: false }, 'guide']],
});
```

The removal and insertion commit together. An optional third tuple value names
a surviving key to insert before; omit it to append. A removed key can be
reinserted, but gets a new incarnation with no inherited item subscriptions.
Inputs are validated before any change; a batch rollback restores the operation.
The item-update loop above does not change membership and needs no bulk request.

A store and a collection commit independently. Nesting their batches does not
create a transaction spanning both.

## Watch membership

```js
tasks.watchKeys(
  (keys, previous, change) => {
    console.log('Current order:', keys);
    console.log('Last successful delivery:', previous);

    if (change) {
      console.log('Inserted:', change.inserted);
      console.log('Removed:', change.removed);
    }
  },
  { signal: lifetime.signal },
);
```

The first call has `previous` and `change` set to `undefined`. Later calls put
membership metadata in the third position. Membership delivery happens before
item delivery, which happens before collection-wide delivery.

A key removed and reinserted identifies a new item incarnation. Old item
watchers end; a replacement starts with no old subscriptions. The membership
notification lists that key as both removed and inserted, even when its string
and final position are unchanged.

A batch rollback restores a removed item and its subscriptions unless their
own signal aborted. End the row's separate binding lifetime when its
incarnation ends too.

When you are done with the watchers in this chapter, call `lifetime.abort()`.

**Next:** [Rendering lists](list-rendering.md)
