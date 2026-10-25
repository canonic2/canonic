# Capabilities

**Status:** agreed shape, written against the filters example in
`examples/filters/`, whose tests exercise every snippet here. The guarantees
are owned by [state management](../contracts/state-management.md); this page
shows how a capability meets them.

A capability is a factory. It creates one store, keeps it private, and returns
a read-only view of its state, its selections and its named actions. It
receives its I/O as injected functions and touches no DOM.

## A complete capability

```js
import { createStore } from '@canonic2/lattice';

export function createFilters({ save }) {
  const store = createStore({ query: '', locked: false, revision: 0 });
  return {
    state: { get: store.get, watch: store.watch },
    select: {
      query: { fields: ['query'], read: (state) => state.query },
      locked: { fields: ['locked'], read: (state) => state.locked },
      summary: {
        fields: ['query'],
        read: (state) => (state.query === '' ? 'All items' : `Matching “${state.query}”`),
      },
    },
    setQuery(query) {
      store.set({ query });
      return save(query);
    },
    toggleLocked() {
      store.set((state) => ({ locked: !state.locked }));
    },
    touch() {
      store.set((state) => ({ revision: state.revision + 1 }));
    },
  };
}
```

The example's own file adds JSDoc typedefs for the state, the selections and
the capability, so callers and bindings are type-checked.

## State ownership

- **One store, kept private.** The capability creates its store and never
  returns it. Consumers get `state`, holding only `get` and `watch`, so no code
  outside the capability can call `set` or `batch`.
- **One owner per fact.** Each fact is one field: the query, the lock, the
  revision. The fields show it as typed and as saved; neither element keeps a
  copy in state.
- **Data only.** The state holds strings, booleans and numbers. Elements,
  observers and controllers stay with the code that owns them.

## Selections

A selection names the fields it reads and derives one value from them. The
capability exports every selection a consumer needs, including derived text
such as `summary`; a binding never derives values itself.

- Declare exactly the fields `read` uses. `summary` reads only `query`, so
  writing `locked` or `revision` never calls it.
- Prefer a `read` that returns a primitive or a value already in state, as all
  three do; that needs no custom `equals`.
- Check every selection with the declaration check, with snapshots that reach
  each branch of `read`. The example's test does this in Node:

```js
import { checkSelection } from '@canonic2/lattice/testing';

const cases = {
  empty: { query: '', locked: false, revision: 0 },
  searching: { query: 'acme', locked: true, revision: 3 },
};

const { select } = createFilters({ save: () => {} });
const result = checkSelection('summary', select.summary, cases);
// Throws if read touched an undeclared field;
// result.unobserved lists declared fields no case reached.
```

## Named actions

Actions are the only way state changes, and each one says what the user or the
system meant: `setQuery`, `toggleLocked`, `touch`. An action that changes
several fields commits once, with one patch or one `batch`.

- A patch function reads the current snapshot, as `toggleLocked` does, instead
  of a value cached elsewhere.
- Writing a value equal to the current one changes nothing and notifies
  no one; an action does not need to compare first.
- Writes are synchronous: every bound element shows the new value before the
  action returns.

## Failures

An action writes its state first, then performs its side effects. `setQuery`
sets the query, then returns whatever `save` returns. If `save` rejects:

- the query stays set, because a committed write is never undone by a later
  failure;
- the rejection reaches the caller, which decides how to present it. The
  capability does not log or report it itself.

When the caller is a binding, it presents the failure through its `report`
port; see [bindings](bindings.md#reporting-failures). A test or a command that
calls the same action handles the rejection its own way.

A capability that holds timers, pending requests or listeners also returns a
`dispose()` that releases them; the filters capability holds none, so it has
no `dispose()`.
