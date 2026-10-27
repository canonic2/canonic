# Collection API

Import `createCollection` and its types from `@canonic2/lattice/collection`.

## `createCollection(entries, options?)`

Accepts an iterable of `[key, item]` pairs and copies each item into a
shallowly frozen record.

```js
const tasks = createCollection([['guide', { text: 'Write the guide', done: false }]]);
```

Keys must be unique strings. Items must be non-null objects without symbol
keys. Each item's own enumerable string keys establish that item's fields.

`options.report` has the same delivery-error policy as
[store options](store.md#createstoreinitial-options).

## Reading

| Member     | Result                                                                        |
| ---------- | ----------------------------------------------------------------------------- |
| `keys()`   | Frozen ordered key array, retained by identity until membership/order changes |
| `get(key)` | Frozen item record, or `undefined` when missing                               |
| `has(key)` | Whether the key exists                                                        |
| `size`     | Current item count                                                            |

Changing an item does not replace other item records or the keys array.
There is no immutable whole-collection snapshot.

## Mutations

| Method                       | Behavior                                                                      |
| ---------------------------- | ----------------------------------------------------------------------------- |
| `update(key, patch)`         | Patch one record with an object or function of its current record             |
| `updateMembership(changes)`  | Apply many removals/insertions atomically with one new frozen keys array      |
| `insert(key, item, before?)` | Insert a new incarnation, before another existing key or at the end           |
| `remove(key)`                | Remove an item and end its item subscriptions when removal commits            |
| `reorder(keys)`              | Apply a permutation of the current keys                                       |
| `batch(fn)`                  | Publish synchronous writes as one commit, with rollback and nested savepoints |

`update` reads each patched value once, compares with `Object.is` and is
silent for unchanged values. Missing keys, unknown fields, invalid patches
and writes during patch resolution throw without publishing partial changes.

Duplicate insertion, an unknown `before` key, removal of a missing key and
an invalid permutation throw `TypeError`. Reordering to the existing order is silent.

A batch defers notifications, not writes. A thrown callback restores its
savepoint. Returning a thenable rolls back and throws. A batch with no net
membership, order, incarnation or field change delivers nothing.

### `updateMembership({ remove?, insert? })`

`remove` contains existing unique string keys. `insert` contains tuples
`[key, record, before?]`. The optional anchor must be an existing key that
survives this call; omitting it or passing `undefined` appends. Insertions at
the same anchor retain supplied order. An insertion cannot target another
insertion as its anchor or overwrite a surviving key.

```js
tasks.updateMembership({
  remove: ['guide'],
  insert: [['guide', { text: 'Rewrite the guide', done: false }]],
});
```

Removing and reinserting a key creates a new incarnation and ends old item
subscriptions on commit. Inputs are validated and copied before mutation;
invalid input or a throwing getter leaves no partial operation. Input getters
may read the collection but cannot write it.

The method returns `undefined`, commits once outside a batch, and joins an
outer batch with its normal rollback guarantees. Reads after the call see the
complete change immediately. Empty requests are silent and preserve keys
identity. Use this method instead of repeated membership operations when one
action should construct the next order only once.

## Item selections

`ItemSelection<T, V>` uses `fields`, `read(item)` and optional `equals`.
Fields are non-empty and validated against the watched item's record.
The reader receives `Readonly<T>`.

```js
const done = { fields: ['done'], read: (item) => item.done };
tasks.watchItem('guide', done, (value, previous) => {}, { signal });
```

`watchItem(key, selection, run, options?)` calls immediately, then for changes
to that item's declared fields whose result differs under equality.

The subscription belongs to the incarnation currently at that key. A committed
removal ends it. Reinserting the same key starts a new incarnation with no old
subscriptions. Rolling back a removal restores the original incarnation and
subscriptions unless their own signal aborted.

## Collection selections

`CollectionSelection<T, V>` uses `fields`, `read(view)` and optional `equals`.
Its field list may be empty. It names item fields read across the collection.

`watch(selection, run, options?)` considers the selection on membership/order
changes or changes to a declared field in any item. The result comparison
defaults to `Object.is`.

The `CollectionView<T>` passed to the reader exposes only traversal: `keys`,
`get`, `has` and read-only `size`. An evaluated aggregate can still scan every
item. Collection-wide fields are validated as strings; there is no shared
runtime item schema.

## Incremental aggregates

`watchAggregate(selection, run, options?)` maintains an order-independent,
reversible reduction. It returns `undefined` and follows watcher signal ownership.

`AggregateSelection<T, C, V>` describes:

| Member                        | Contract                                                         |
| ----------------------------- | ---------------------------------------------------------------- |
| `fields`                      | Item fields used by `read`; copied on registration; may be empty |
| `read(item)`                  | Pure synchronous contribution `C` from `Readonly<T>`             |
| `initial()`                   | Fresh empty accumulator `V`                                      |
| `add(value, contribution)`    | Pure accumulator including one contribution                      |
| `remove(value, contribution)` | Pure accumulator excluding that contribution; reverses `add`     |
| `equals(next, previous)`      | Optional synchronous delivery equality; defaults to `Object.is`  |

Registration visits current keys once and calls `run(value, undefined)`.
Relevant commits use old/new records and publish at most once. Unrelated fields,
reorder-only commits and rolled-back changes do not maintain the aggregate.
No per-item contribution cache is retained.

Computational callbacks run under a same-collection write guard. Promise/thenable
results throw during initialization or report during maintenance; actual Promise
rejections are observed. Maintenance failure discards partial work and rebuilds
on the next relevant commit. Equality or delivery failure keeps healthy state
and the last successful delivery history. Abort stops subsequent callbacks.

Results preserve committed prefixes during reentrant writes. Their callbacks
share registration order with ordinary `watch` in the collection-wide phase.
Fields are validated as strings, without a shared runtime item schema. Values
and contributions are immutable by contract; no deep freeze is performed.

See [Maintain counts and sums](../guide/aggregates.md) for a complete example,
arithmetic constraints and when to keep a scanning selection.

## Membership subscriptions

`watchKeys(run, options?)` calls `run(keys, previous, change)` immediately and
after structural commits.

`previous` and `change` are initially `undefined`. Later, `previous` contains the
last successfully delivered keys; a thrown callback does not advance it.
`change` has `inserted` and `removed` key arrays.

Replacement of an existing key appears in both arrays and notifies even when
the final key strings and order match. A pure reorder has empty arrays.

Keys reflect the latest state when notified. Metadata describes its queued
commit and is not a complete diff from `previous` when callbacks queue more
writes.

## Delivery and lifetimes

A commit delivers membership first, changed items second, and collection-wide
subscriptions (`watch` and `watchAggregate`) last, in registration order.
Changed items follow first-change order, with subscription order inside each
item. Each considered subscription is evaluated at most once per commit.

Value callbacks receive `(value, previous)`. Equality compares
`equals(next, previous)`. The initial call does not use equality. Initial failures
throw and leave no subscription; later failures use `report`.

Every watch method accepts optional `{ signal }` and returns `undefined`.
Aborting stops delivery and releases its subscription. Without a signal,
collection-wide subscriptions last as long as the collection; item subscriptions
also end on committed removal.

Watch methods reject calls inside batches. Key and selection validation may
throw before an already-aborted signal is checked. Writes during delivery queue
notifications, and the outermost write drains them before returning.

## Reader types

`CollectionReader<T>` exposes traversal and the four watch methods without
mutations. Forward `size` through a getter when constructing an application
reader.

`CollectionView<T>` exposes traversal only and is supplied to derivations.
Both types describe access without creating a runtime wrapper.

**Guide:** [Collections](../guide/collections.md) ·
[Rendering lists](../guide/list-rendering.md)
