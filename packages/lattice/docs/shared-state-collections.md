# Items in one store field

Keep items in an ordinary store field when consumers need a whole-state
snapshot or the item set is small enough that replacement and notification
costs fit your application. This uses `createStore`; it is not a separate API.

## Replace nested records

```js
import { createStore } from '@canonic2/lattice';

const store = createStore({
  byId: {
    t1: { label: 'Write the plan', done: false },
    t2: { label: 'Review', done: false },
  },
});

store.set((state) => ({
  byId: {
    ...state.byId,
    t1: { ...state.byId.t1, done: true },
  },
}));
```

Do not mutate `store.get().byId.t1`. The store freezes only the outer
snapshot, and nested mutation would bypass notifications and change data held
by earlier snapshots.

If an action changes many items, build the replacement map once and pass it
in one patch. Wrapping many whole-map copies in a batch reduces delivery
count, but does not remove those copies.

## Understand the selection cost

All row selections over `byId` declare the same field. Replacing `byId`
considers every one of them, even when each reader returns only its own item.
Unchanged selected references can suppress callbacks, but their readers still
run. Equality does not eliminate the work needed to obtain those results.

This is useful when complete snapshots and simple ownership matter more than
isolated item-update costs. For large, frequently edited item sets, use a
[keyed collection](keyed-collections.md), which avoids replacing the entire
map on each item update. Its tradeoff is that it has no immutable snapshot of
the whole collection at one commit.

List presentation remains a separate choice: either model can use
[fully mounted rows](mounted-lists.md) or application-owned
[windowing](windowing.md).
