# Lifetimes

A view can disappear while its state remains useful. A dialog closes; a row
leaves a visible range; an application replaces one panel with another.

Give the view one explicit lifetime so its subscriptions and external
listeners end together.

## Own the view lifetime

The entry point owns an `AbortController` and passes its signal to bindings
and listeners:

```js
const lifetime = new AbortController();

task.state.watch(
  task.select.status,
  (value) => {
    status.textContent = value;
  },
  { signal: lifetime.signal },
);

button.addEventListener(
  'click',
  () => {
    task.setDone(true);
  },
  { signal: lifetime.signal },
);

// When this view is finished:
lifetime.abort();
status.remove();
button.remove();
```

This fragment uses a task capability, a status element and a button already
created by the entry point. Abort first, then remove presentation. Later
actions can still update the capability, but they cannot update this ended view.

Aborting again is harmless. To show a new view, create a new controller and
new bindings.

## Disconnection is different from disposal

Removing a custom element from the DOM may be temporary. Moving a panel or
detaching a list should not necessarily throw away drafts or application state.

An element's **connection lifetime** owns resources it needs while connected.
The **view lifetime** owns the bindings established by application composition.

Keep those owners distinct. A temporarily disconnected element can retain its
inputs and nodes while releasing document listeners and observers. Final view
disposal ends its bindings too.

The [task-list example](../../examples/tasks/README.md) demonstrates this
distinction, including retained drafts across temporary detach.

## Connection resources

Acquire listeners on the host, document or window during connection. Use a
fresh controller on each connection and release it on disconnection.

For example, a selectable row can handle Enter on the row host while leaving
keyboard events from its internal controls alone. These are members of that
row's class; its caller gives the host an appropriate `tabindex`:

```js
#connection = null;

connectedCallback() {
  if (!this.isConnected) return;

  this.#connection?.abort();
  const own = new AbortController();
  this.#connection = own;

  try {
    this.addEventListener('keydown', event => {
      if (event.composedPath()[0] !== this || event.key !== 'Enter') return;
      event.preventDefault();
      this.dispatchEvent(new CustomEvent('acme-task-select', {
        bubbles: true,
        composed: true,
      }));
    }, { signal: own.signal });
  } catch (error) {
    own.abort();
    this.#connection = null;
    throw error;
  }
}

disconnectedCallback() {
  const own = this.#connection;
  this.#connection = null;
  own?.abort();
}

connectedMoveCallback() {}
```

An element with listeners only on its own retained shadow nodes needs no
connection controller just for those listeners. Install them once in the
constructor, as the task toggle does. A handler that emits user intent checks
`isConnected` before dispatching.

For observers, disconnect explicitly when the connection's signal aborts.
For deferred work, capture that connection's signal and check it before
applying the result. Checking a newer connection's controller would allow stale
work to reach a reconnected element.

## Roll back partial setup

A binding can fail on its first assignment. A view can fail after creating
some of its bindings. The owner releases what it already acquired before
propagating the original failure.

The [filters entry](../../examples/filters/app.js) is a complete example.
It ends its internal binding lifetime, then attempts all owned node removals.
Cleanup failures use its reporter instead of replacing the setup error.

Cleanup should mark the lifetime ended before releasing resources and continue
after an individual release fails. Explicit disposal can report or aggregate
cleanup failures according to its contract; abort-triggered cleanup reports
them through its owner's error policy.

## State that outlives a view

Stores have no `dispose()` method. End their watchers with signals and release
the store when its capability is no longer needed.

A capability that owns requests, timers or other resources should expose an
application `dispose()` method for those resources. That method is part of
your capability, not something `createStore` adds.

**Next:** [Collections](collections.md)
