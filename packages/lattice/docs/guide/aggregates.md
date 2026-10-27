# Maintain counts and sums

Use `watchAggregate` when a collection summary can be updated by adding and
removing each changed item's contribution. A task count or an integer total
does not need to rescan thousands of records after one edit.

## Count open tasks

This complete module prints `2`, then `1`, then `0`:

```js
import { createCollection } from '@canonic2/lattice/collection';

const tasks = createCollection([
  ['guide', { text: 'Write the guide', done: false }],
  ['review', { text: 'Review it', done: false }],
]);
const lifetime = new AbortController();

const open = {
  fields: ['done'],
  read: (task) => (task.done ? 0 : 1),
  initial: () => 0,
  add: (total, contribution) => total + contribution,
  remove: (total, contribution) => total - contribution,
};

tasks.watchAggregate(open, (count) => console.log(count), {
  signal: lifetime.signal,
});
tasks.update('guide', { done: true });
tasks.updateMembership({
  remove: ['review'],
  insert: [['publish', { text: 'Publish it', done: true }]],
});
lifetime.abort();
```

Registration visits current items once. A later `done` change projects that
record's old and new values, removes the old contribution and adds the new one.
Editing `text` does no reduction work. Reordering alone also does no work.
There is no per-item contribution cache.

A batch maintains only its net changes and delivers at most one result. A
rolled-back batch does not update the summary. `equals`, which defaults to
`Object.is`, suppresses delivery when the completed result is unchanged.

## Choose a reversible reduction

`read` receives one immutable item, rather than a collection traversal view.
Declare every field it reads. Check the declaration in tests using the existing
`checkSelection(name, { fields, read }, cases)` helper.

`add` and `remove` must be pure, order-independent operations. Removing a
contribution must reverse adding it. They can return structured immutable values,
such as `{ sum, count }`, but must never mutate an earlier accumulator.

For an integer total, project the amount and add/subtract it. Validate the
numeric domain in your reducer: contributions and every intermediate total must
remain safe integers. A safe final value alone is insufficient. Floating-point
addition is not exactly reversible, so this API does not promise that an
incremental float sum equals an ordered full recomputation.

Use ordinary [`watch`](../api/collection.md#collection-selections) for an
order-dependent result, arbitrary joins or min/max without removal bookkeeping.
Scanning can also be simpler for small lists or infrequent changes. Incremental
maintenance pays an initialization cost and invokes your projection/reducer
functions; choose it when it avoids useful work, not merely because it is available.

For membership size alone, use `size` with a membership subscription. An empty
aggregate field list is allowed for other constant-contribution reductions.

## Errors, delivery and lifetime

All computational callbacks must return synchronously. A Promise or thenable
result is rejected with `TypeError`. Actual Promises have their rejection
observed, but their eventual value is never applied. Keep asynchronous work in
application actions rather than reducers.

Initialization errors throw without leaving a subscription. After registration,
a projection or reduction failure goes to the collection's `report` callback.
The item change still commits; a partial summary is not delivered. On the next
relevant commit, the aggregate rebuilds from all current records and can resume
delivery. This full scan is error recovery, not normal maintenance.

An `equals` or delivery callback failure also reports, while healthy accumulator
maintenance continues. `previous` remains the last successfully delivered value.

When callbacks trigger more writes, incremental results preserve each commit's
value. A count transition `0 → 1 → 2` delivers `1` followed by `2`, even when
ordinary scanning watchers read a newer live view. Aggregate and scanning
callbacks share the collection-wide phase in registration order.

Pass an `AbortSignal` to end the subscription and release its callbacks and
retained values. Without one, it lasts with the collection. Set up watchers
outside a batch.

See the [collection reference](../api/collection.md#incremental-aggregates) for
the public types and method contract.
