# Composition

**Status:** agreed shape, written against the filters example's entry,
`examples/filters/app.js`, and its page, `index.html`; the example's browser
tests exercise mounting and teardown. The guarantees are owned by
[bindings](../contracts/bindings.md#lifetime) and
[resource lifecycle](../contracts/resources.md).

An entry point composes the parts: it defines elements, creates capabilities
with their ports, creates elements, binds them for a lifetime and ends that
lifetime. Capabilities, bindings and elements never compose themselves.

## A complete entry

```js
import { bindFilterField, bindFilterSummary } from './bindings.js';
import { AcmeFilterField, defineAcmeFilterField } from './filter-field.js';
import { AcmeFilterSummary, defineAcmeFilterSummary } from './filter-summary.js';
import { createFilters } from './filters.js';

export function mountFilters(container, { signal, count = 3, save = () => {}, report = reportError }) {
  defineAcmeFilterField();
  defineAcmeFilterSummary();
  const filters = createFilters({ save });
  if (signal.aborted) return filters;

  const summary = new AcmeFilterSummary();
  const fields = Array.from({ length: count }, (_, index) => {
    const field = new AcmeFilterField();
    field.setAttribute('label', `Filter ${index + 1}`);
    return field;
  });
  const lifetime = { signal, report };
  for (const field of fields) bindFilterField(field, filters, lifetime);
  bindFilterSummary(summary, filters, lifetime);
  container.append(summary, ...fields);
  signal.addEventListener('abort', () => {
    summary.remove();
    for (const field of fields) field.remove();
  }, { once: true });
  return filters;
}
```

## Creation

1. **Define elements** with their `defineX` functions. Importing a module
   defines nothing, so the entry point decides which elements exist and under
   which registry.
2. **Create capabilities** and inject their I/O: here, `save`. The entry point
   is where real ports, such as storage or a network client, meet the
   capability; tests inject fakes in the same place.
3. **Create elements** and set their static attributes, such as each field's
   label.
4. **Bind** each element to its capability for the lifetime. Bindings apply
   initial values synchronously, before the elements are connected.
5. **Connect** the elements. An element acquires its own connection resources,
   such as the field's size observer, when it connects.

The page that runs the example does only this and wires two buttons:

```js
const lifetime = new AbortController();
const filters = mountFilters(document.getElementById('app'), {
  signal: lifetime.signal,
  save: (query) => localStorage.setItem('acme-filter', query),
});
document.getElementById('lock').addEventListener('click', () => filters.toggleLocked(), {
  signal: lifetime.signal,
});
document.getElementById('teardown').addEventListener('click', () => lifetime.abort(), {
  once: true,
});
```

## Lifetimes

A lifetime is an `AbortSignal` owned by whoever created what it serves: the
page here, a dialog's open period, or one item of a collection. Everything
created for it takes that signal: bindings, their subscriptions and listeners,
and the entry's own listeners. Nothing is resumed after its lifetime ends;
showing the same thing again creates a new lifetime and new bindings.

## Teardown

Aborting the lifetime ends everything at once:

- every binding releases its subscriptions and listeners, so later actions
  assign nothing and element events call no action;
- the entry removes its elements, and each element releases its connection
  resources as it disconnects;
- the capability keeps working for anyone still holding it: an action still
  updates state and calls its ports, but no element shows the result.

Aborting again does nothing. The example's tests check each of these, and the
[full-flow record](../../research/full-flow.md) checked that no listener or
node remains after repeated mounting and teardown.
