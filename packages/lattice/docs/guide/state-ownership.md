# Choosing state ownership

Start with the fact you need to represent. Who may change it? Who needs to
observe it? Which changes must arrive together?

The same Lattice primitives support several answers. You do not need to choose
a different model for every feature.

## State inside an element

Keep an interaction local when only the element needs it: an open disclosure,
hover feedback, or an unsubmitted draft.

A disclosure can hold a private `#open` field and update its own panel. It
needs no store just because its markup is a Web Component.

Move the decision into an application capability when other consumers need
to observe it, actions must validate it, or it belongs in persistence. The
component then receives an input and emits intent.

## Related fields in one store

Use a store when fields form one related record:

```js
import { createStore } from '@canonic2/lattice';

const editor = createStore({
  text: 'Write the guide',
  done: false,
  saving: false,
});
```

One action can update several fields in one patch or batch. Consumers that
need a complete immutable record can hold `editor.get()`.

The changing write copies the outer record, and delivery walks subscriptions.
That is a useful starting point for small application state.

## Small item sets in a shared snapshot

Sometimes a whole-state snapshot is more useful than independent item updates.
You can keep a small item set in one store field:

```js
import { createStore } from '@canonic2/lattice';

const project = createStore({
  tasks: {
    guide: { text: 'Write the guide', done: false },
    review: { text: 'Review the examples', done: false },
  },
});

project.update((state) => ({
  tasks: {
    ...state.tasks,
    guide: { ...state.tasks.guide, done: true },
  },
}));
```

This preserves one complete project snapshot. It also copies the item map
when one task changes. Every selection reading `tasks` declares that same
field and can be evaluated on the replacement.

For a bulk change, build the map once and publish one patch. Batching many
whole-map copies does not remove the copies.

## Independent item capabilities

An editor session can be more than a row: it may own requests, timers and
actions of its own. Such an item can justify a separate capability and store.

```js
const first = createStore({ text: 'First draft', dirty: false });
const second = createStore({ text: 'Second draft', dirty: false });

second.update({ dirty: true });
```

The first store's watchers are not involved. Each owner can have its own
lifetime and named actions.

The tradeoff is independent commits. Updating many stores in a loop delivers
each update separately. Nesting their batches does not give the group a
shared transaction, and repeated aggregate work can become expensive.

## Many independently changing records

Use a keyed collection when many items change independently but membership,
order and bulk actions belong to one owner.

A task edit replaces one item record. A batch can complete every task with one
final delivery. Use `updateMembership` for many membership changes with one order
construction, and [incremental aggregates](aggregates.md) for reversible counts
and sums. Ordinary `watch` still supports arbitrary scanning derivations.

A collection has no whole-collection immutable snapshot. Build one deliberately
from `keys()` and `get(key)` when a consumer such as persistence needs it.

## Keep presentation a separate choice

State ownership does not decide how much DOM to mount. A keyed collection can
drive a small fully mounted list or a windowed one. A shared snapshot can also
drive retained rows.

Choose the simplest combination that meets the behavior and measured workload:

| Need                                        | Start with               |
| ------------------------------------------- | ------------------------ |
| Private interaction                         | Element-local state      |
| Related scalar fields                       | One store                |
| Complete snapshot of a small item set       | Items in one store field |
| Independent item behavior and resources     | A capability per item    |
| Independent record updates and bulk actions | A keyed collection       |

Avoid maintaining two independently writable copies of the same fact. Derive
display data with selections instead of adding another owner.

**Next:** [Large lists](windowing.md)
