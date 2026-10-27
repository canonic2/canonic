# Bindings

**Status:** agreed shape, written against the filters example in
`examples/filters/bindings.js`, with browser coverage for the common path.
The [contract's coverage map](../contracts/bindings.md#coverage-and-gaps)
identifies remaining acceptance cases. The guarantees are owned by
[bindings](../contracts/bindings.md); Lattice exports no binding API.

A binding connects one element to one capability for one lifetime. It assigns
selected values to the element's inputs and turns the element's events into
action calls. It is the only code that knows both.

## A complete binding

```js
export function bindFilterField(element, filters, { signal, report }) {
  if (signal.aborted) return;
  const failed = new AbortController();
  const own = AbortSignal.any([signal, failed.signal]);

  const present = (error) => {
    if (own.aborted) return reportError(error);
    try {
      report(error);
    } catch (reportFailure) {
      reportError(reportFailure);
    }
  };

  const { state, select } = filters;
  try {
    state.watch(select.query, (query) => (element.value = query), { signal: own });
    state.watch(select.locked, (locked) => (element.disabled = locked), { signal: own });
  } catch (error) {
    failed.abort();
    throw error;
  }
  element.addEventListener(
    'acme-filter-input',
    (event) => {
      try {
        const result = filters.setQuery(event.detail.value);
        if (typeof result?.then === 'function') Promise.resolve(result).then(undefined, present);
      } catch (error) {
        present(error);
      }
    },
    { signal: own },
  );
}
```

The summary's one-subscription binding has no events and no earlier acquired
resource to roll back if its initial assignment throws:

```js
export function bindFilterSummary(element, filters, { signal }) {
  if (signal.aborted) return;
  filters.state.watch(filters.select.summary, (text) => (element.text = text), { signal });
}
```

## Inputs

Each subscription assigns one selected value to one public input of the
element: `value`, `disabled`, `text`. The assignment is the whole callback; any
derivation belongs to a selection of the capability. The binding never reads or
writes the element's shadow tree.

`watch` runs each callback once immediately, so the element shows the current
state as soon as the binding returns, and again after each commit that changes
the selected value. A write that changes nothing the binding selects assigns
nothing.

## Events

A listener reads the event's documented `detail` and calls one action. It
never writes state directly and never writes an input back into state: the
value flows from state to the element, and intent flows from the element to an
action, which may change state.

## Reporting failures

The binding calls the action inside `try`/`catch`, so a synchronous throw is
presented. When the action returns something with a `then` method, the binding
passes it through `Promise.resolve` and attaches one rejection handler, so a
rejection is presented once and never becomes unhandled; a synchronous action
allocates no promise.

`present` hands a failure to the binding's `report` port while the binding is
live. After its signal aborts there is no UI to present in, so a late failure
goes to the platform's `reportError` instead. If `report` itself throws, that
error goes to `reportError` too.

## Setup rollback

Subscriptions and the listener all use `own`: the caller's signal combined with
an internal `failed` controller. If an initial assignment throws while the
binding is being set up, the binding aborts `failed`, which releases every
subscription made so far, and rethrows. The caller's lifetime is never aborted
by the binding's own failure.

When the caller's signal aborts, `own` aborts with it: every subscription and
the listener are released at once, and aborting again does nothing.

## Cost

Each field binding allocates the internal controller, the combined signal,
`present`, two subscriptions and one listener. The
[full-flow research](../../../research/full-flow.md) owns measurements against
hand-written wiring and their conditions. Single-abort cleanup and
setup rollback remain required; those measurements do not automatically apply
to later binding or lifecycle changes.
