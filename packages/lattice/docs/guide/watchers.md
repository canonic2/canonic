# Watchers

A watcher connects a selected value to something that uses it. In a browser,
that is often one public input or one text node.

```js
const lifetime = new AbortController();

task.watch(
  status,
  (value) => {
    output.textContent = value;
  },
  { signal: lifetime.signal },
);
```

This fragment uses the `task` and `status` from the previous chapter, and
`output` is the element that displays the message. The callback receives its
first value immediately.

## Current and previous values

Every value watcher receives `(value, previous)`:

```js
import { createStore } from '@canonic2/lattice';

const task = createStore({ done: false });
const done = { fields: ['done'], read: (state) => state.done };

task.watch(done, (value, previous) => {
  console.log(value, previous);
});
// false, undefined

task.update({ done: true });
// true, false
```

The previous value is the last one delivered successfully. Initial delivery
does not call the selection's equality function.

## End a subscription

Give a watcher the lifetime of the view it serves:

```js
const lifetime = new AbortController();

task.watch(done, (value) => console.log(value), {
  signal: lifetime.signal,
});

lifetime.abort();
task.update({ done: false }); // This watcher receives nothing.
```

`watch` returns nothing. The signal ends the subscription; there is no separate
unsubscribe function. An already-aborted signal acquires no subscription and
calls no reader or callback. Argument validation can still reject an invalid
selection.

Without a signal, a store watcher lasts as long as the store. Item watchers
also end when their item's removal commits.

## Understand timing

Delivery is synchronous. Outside a batch, a changing update notifies its
watchers before returning. In a batch, delivery waits for the outermost
callback to finish.

A write made by a watcher becomes current immediately, but its notifications
queue behind the delivery in progress. They drain before the outermost write
returns. Readers always receive the latest state when notified; they are not
replaying a history of immutable commit events.

Register watchers outside batches. Their immediate callback would otherwise
show pending state that a rollback could discard.

## Handle failures

If the initial reader or callback throws, `watch` throws and leaves no
subscription.

After setup, a subscriber failure goes to the owner's `report` option. The
commit stands and the other subscriptions keep receiving their values:

```js
const task = createStore(
  { done: false },
  {
    report: (error) => console.error('Task view failed', error),
  },
);
```

A failed callback stays subscribed and keeps its last successful value as
`previous`. The default reporter uses the platform's `reportError`, or surfaces
the error from a microtask when that API is unavailable.

An action that calls `update` handles rejected writes. The owner's reporter
handles errors from callbacks observing an accepted write. Keep those two
failure paths distinct when deciding how your UI presents errors.

**Next:** [Components](components.md)
