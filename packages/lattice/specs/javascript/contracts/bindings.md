# Bindings

## Purpose and status

A binding is the seam between application state and an element. It applies
selected state to an element's public inputs and turns the element's events
into calls to the capability's actions, for one lifetime. It is the only code
that knows both a store and an element.

```text
store    state, actions, selections          (state management)
binding  selected values → inputs; events → actions; one lifetime
element  renders its inputs, emits intent    (element lifecycle, DOM updates)
```

**Status:** agreed application convention, implemented by the filters and tasks
examples using store and collection subscriptions. Lattice exports no binding API. A helper
is added only under the conditions in [decisions](#decisions).
[Resource lifecycle](resources.md) owns shared cleanup guarantees;
[state management](state-management.md) owns subscriptions, equality and delivery.

## Shape

Binding signatures use `bindX(target, capability, { signal, report })`. An
item binding places its key after the capability and before the options.
Bindings with several presentation targets group them in the first argument.
A binding with no action failures to present accepts `{ signal }` without a
`report` port. A bare positional signal is not the authoring convention.

A binding is a function that receives one element, one capability, the
`AbortSignal` of the lifetime it serves and a `report` port that presents
action failures for that lifetime:

```js
/**
 * Connects a notifications toggle to the notifications capability until `signal` aborts.
 * @param {AcmeToggle} element The toggle it drives.
 * @param {Notifications} notifications The capability it shows and changes.
 * @param {object} lifetime The binding's lifetime and failure presenter.
 * @param {AbortSignal} lifetime.signal Ends the binding when it aborts.
 * @param {(error: unknown) => void} lifetime.report Presents a failed action;
 *   synchronous and non-throwing.
 * @returns {void}
 */
export function bindToggle(element, notifications, { signal, report }) {
  if (signal.aborted) return;
  const failed = new AbortController();
  const own = AbortSignal.any([signal, failed.signal]);

  /**
   * Presents a failure while the binding is live; otherwise hands it to the platform.
   * @param {unknown} error The action's failure.
   * @returns {void}
   */
  const present = (error) => {
    if (own.aborted) return reportError(error);
    try {
      report(error);
    } catch (reportFailure) {
      reportError(reportFailure);
    }
  };

  const { state, select } = notifications;
  try {
    state.watch(select.on, (on) => (element.on = on), { signal: own });
    state.watch(select.locked, (locked) => (element.disabled = locked), { signal: own });
    state.watch(select.label, (label) => (element.label = label), { signal: own });
  } catch (error) {
    failed.abort();
    throw error;
  }
  element.addEventListener(
    'acme-toggle',
    (event) => {
      try {
        const result = notifications.setOn(event.detail.on);
        if (typeof result?.then === 'function') Promise.resolve(result).then(undefined, present);
      } catch (error) {
        present(error);
      }
    },
    { signal: own },
  );
}
```

## Responsibilities

- **Inputs are assignments.** Each subscription assigns one selected value to one
  public property or attribute of the element. Any derivation, including
  negation, formatting or combining fields, is a selection of the capability, so
  the rule is testable without the DOM and shared by every consumer.
- **Events are action calls.** A listener reads the event's documented `detail`
  and calls one action. It does not change state directly, decide product
  rules or call `update` on a store.
- **No two-way binding.** A value flows from state to an input; intent flows from
  an event to an action, which may change state, which flows back. An element
  input is never written back into state by the binding.
- **Public surface only.** A binding uses the element's documented properties,
  attributes and events. It never reads or writes the element's shadow tree,
  private fields or internal nodes.
- **No state of its own.** A binding holds no values between calls. What it needs
  to remember belongs in the capability's store or in the element.
- **One owner per DOM effect.** Each element input, and each attribute such as
  `inert`, `hidden` or `aria-busy` written on a host element, has exactly one
  binding that writes it. Two bindings never write the same input.
- **Same document only.** A binding connects a store and an element in the same
  document. State reaches another document, such as an iframe, through that
  capability's messages, and a binding in that document.

## Lifetime

- A binding acquires nothing when its signal is already aborted.
- Every subscription and listener it creates uses its signal. Aborting the signal
  releases all of them at once; aborting again does nothing.
- The signal belongs to the lifetime of what the binding serves: the page, a
  dialog's open period, or the presence of one item in a keyed collection. The
  code that creates that lifetime creates the binding and ends it.
- Reconnecting an element, or recreating a dialog or item, creates a new binding
  with a new signal. A binding is never resumed.
- For an item of a [keyed collection](collections.md), removing the item ends
  its item subscriptions when the removal commits, before the membership
  binding aborts the row's lifetime; that abort then finds them already ended.
  A key removed and reinserted is a new item: the membership binding gives it
  a new row and binding.
- If acquiring a subscription or applying an initial value throws, the binding
  releases everything it already acquired before the error propagates. It does
  so with an internal controller combined with the given signal, as in the
  example, so a failure never aborts the caller's lifetime.
- After the signal aborts, no subscription or listener of the binding runs,
  including subscriptions for a write that was being delivered when it aborted,
  as state management's delivery rules guarantee. The one exception is the
  rejection handler of an action started before the abort: it still observes
  the failure, and only hands it to `reportError`, as
  [errors](#errors-and-asynchronous-actions) describes. It never touches the
  element or calls `report`.

## Ordering

- Initial values are applied synchronously when the binding is created, in the
  order its subscriptions are declared, before its event listeners exist.
  Setting an input does not emit a user-action event, so the initial application
  cannot call an action.
- After a commit, the binding's considered subscriptions run within that
  commit's synchronous delivery, in declaration order. The element receives
  every changed input of the commit before the outermost `update` or `batch`
  returns. When the commit was made by a subscriber during another delivery, the
  `update` that made it returns first, as state management's delivery rules
  describe; the element still receives the inputs before the outermost write
  returns.
- A binding introduces no scheduling. When an element's rendering benefits from
  batching, such as one layout pass per frame, the element or an explicitly
  owned scheduler handles it; state acceptance and input assignment stay
  synchronous.

## Errors and asynchronous actions

- The capability owns recovery, as state management specifies: it records the
  failure in its state, releases resources and applies its recovery policy.
  What it does not recover from reaches the binding as a throw or a rejection.
- The binding owns presentation for actions it triggers from events. It calls
  the action synchronously inside `try`/`catch`, so a synchronous throw is
  presented.
- An action may return a promise, a promise from another realm such as an
  iframe, or any other object with a `then` method. When the result has a `then`
  method, the binding normalizes it with `Promise.resolve(result)` and attaches
  one rejection handler. That calls a thenable's `then` once, turns a throwing
  `then` into a rejection, and presents each failure at most once. A result
  without `then` takes no further step: a synchronous action allocates no
  promise for error handling.
- A failure is presented through `report` only while the binding's signal is
  live. A rejection that settles after the signal aborts is still observed, but
  is passed to the platform's `reportError` instead, so an expired binding never
  notifies a closed dialog or touches disconnected UI, and the failure still
  reaches the application's global error handling.
- `report` is synchronous, returns nothing and does not throw; the code that
  supplies it is responsible for that. If it throws anyway, the binding passes
  that error to `reportError`. A failure never becomes an uncaught exception from
  the listener or an unhandled rejection, and is never swallowed silently.
- A binding does not retry, debounce or deduplicate actions. Those are product
  decisions owned by the capability's actions.
- A setter that throws during delivery is a subscriber error: it goes to the
  store's `report`, the remaining subscriptions are still notified, and the
  commit stands. It never comes out of the `update` that made the commit, so it
  cannot interrupt the action that wrote the state.

## Cost

A binding adds no traversal of its own. Its cost for a commit is the state
management cost of its subscriptions: a field check for each, a `read` and a
comparison for each whose fields changed, then one assignment for each value
that changed.

A binding subscribes with the capability's exported selections and never
declares fields itself. The capability that writes a selection's `read`
declares its `fields` beside it and tests them, so a binding cannot miss an
update by declaring the wrong fields.

Selectivity is an optimization with its own cost. Whatever decides which
subscriptions run must cost less in the intended workload than running them;
a binding adds no filtering, caching or comparison of its own on top of the
store's.

Creating a binding also costs allocations: an internal `AbortController` and a
combined signal for rollback, the `present` function, one subscription per
input and one listener per event. This is the price of single-abort cleanup and
partial-failure rollback. In a keyed collection it is paid once per item, so
collection workloads measure it, as [performance and evidence](performance.md)
describes. A cheaper setup is adopted only with measurements showing the
current one is a material cost, and only if it keeps the same lifetime
guarantees.

## What elements own

Elements are used without bindings too, by other hosts, tests and
documentation examples, so their own inputs must be cheap regardless of how
often they are set:

- A setter compares the new value with the value it holds, or with a live
  control's current value for inputs a person can edit, and returns without a
  DOM write when they are equal, as [DOM updates](dom-updates.md) specifies.
- An element keeps its meaningful input values and the node references it
  patches, and updates only the nodes an input affects. It does not read the DOM
  to recover a value it already owns.

A binding does not depend on these guards for correctness, and does not compare
values on the element's behalf beyond the store's equality.

## Testing

Browser tests use real elements in Chromium:

| Case                  | Assertion                                                                                                                                                                             |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Initial application   | Every input has its selected value once the binding returns; no action was called                                                                                                     |
| Selective updates     | A write that changes one selected value assigns only that input; an unrelated write assigns none                                                                                      |
| Events                | Each documented event calls its action with the event's detail                                                                                                                        |
| Abort                 | After abort, writes assign nothing and events call nothing; aborting twice is harmless                                                                                                |
| Already aborted       | A binding created with an aborted signal assigns nothing and installs no listener                                                                                                     |
| Partial failure       | When an initial assignment throws, no subscription or listener remains                                                                                                                |
| Action failure        | A synchronous throw and a rejected promise each reach `report` once; neither produces an uncaught error or unhandled rejection; a synchronous action that succeeds creates no promise |
| Foreign promises      | A rejected promise from another realm, a rejecting thenable and a thenable whose `then` throws each reach `report` exactly once                                                       |
| Late failure          | An action starts, the binding aborts, then the action rejects: `report` is not called, the window receives an `error` event for the failure, and there is no unhandled rejection      |
| Throwing reporter     | When `report` throws, the window receives the platform-reported `error` event for that error, with no error escaping the action listener or becoming an unhandled rejection           |
| Abort during delivery | A binding aborted by an earlier subscriber of the same write receives nothing further                                                                                                 |

Work counts, the number of `read` calls, assignments and listener calls per
commit, are asserted with counters around selections and setters, as state
management's testing describes.

### Coverage and gaps

`examples/filters/filters.browser.test.js` covers initial values, input/event
flow, selective assignments, teardown, rejected saves, throwing reporters,
subscription setup rollback, rollback after a later mounting failure,
continued node cleanup after a release failure and a rejection after teardown.
`examples/tasks/tasks.browser.test.js` covers item binding lifetimes,
incarnation replacement, view disposal, partial row-binding failure and
continued view teardown after presentation disposal fails.

The table above is the acceptance contract, not a claim that every case has a
dedicated test. Explicit foreign-realm promises, unusual thenables, synchronous
action failures, already-aborted binding acquisition and abort by an earlier
subscriber remain cases to exercise at the binding boundary. Primitive abort
tests do not establish an entire binding's behavior. The tasks bindings invoke
synchronous actions; they are not a complete asynchronous action recipe.

## Decisions

- **No binding helper for now.** `state.watch` with a signal and
  `addEventListener` with the same signal already express a binding in a few
  lines; a wrapper would add a layer without removing work. A helper is added
  when repeated bindings across applications show the same shape, and only if
  it adds no traversal, property scanning or scheduling.
- **Assignment only.** Keeping derivations in selections puts every rule in the
  capability, where it is tested in Node, and keeps bindings uniform enough to
  review at a glance.
- **No two-way binding.** Writing inputs back into state would make the element
  a second writer of the fact. Events and actions keep one writer.
- **Bindings separate from DOM updates.** The binding decides which input
  receives a value; the element decides which nodes that input changes. Neither
  reaches into the other.
- **The caller presents failures.** The binding receives `report` because it is
  the caller of event-triggered actions. Tests, commands and workflows call the
  same actions and handle their failures their own way; capabilities leave action-failure presentation to their callers. Primitive
  subscriber errors still use the store or collection's `report` policy.
- **Late failures go to the platform.** Once its lifetime ends, a binding has no
  UI to present a failure in. Handing the failure to `reportError` keeps it
  observed and visible to the application's global error handling without a
  second port in every binding's signature.
