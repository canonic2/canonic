# State management

## Purpose and status

Lattice's state model gives an interface one owner for each fact, explicit
named changes, and updates that reach only the DOM that depends on them. It
has three parts: a store core, the capability stores an application writes on
it, and bindings that connect stores to elements. Elements stay unaware of
stores.

**Status:** agreed contract. The store core is implemented and exported from
the root entry point, and the declaration check from
`@canonic2/lattice/testing`. [Delivery](delivery.md) owns runtime boundaries, and
[resource lifecycle](resources.md) owns shared cleanup guarantees.

## Layers

```text
store    state, actions, selections and transitions; no DOM
binding  selections → element inputs; element events → actions
element  renders its inputs, emits intent; imports no store
```

| Layer            | Provided by                            | Owns                                                                          | Never does                                      |
| ---------------- | -------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------- |
| Store core       | Lattice                                | Snapshots, writes, subscriptions                                              | Product logic                                   |
| Capability store | The application, on the core           | One capability's state, its actions, selections and transition rules          | Touch the DOM, write another capability's store |
| Binding          | The application                        | Applying selected values to element inputs; routing element events to actions | Hold state of its own, decide product rules     |
| Element          | The application or a component library | Rendering its inputs, interaction-local state, emitting events                | Import a store, perform I/O                     |

Entry points create stores, call bindings and pass each binding the
`AbortSignal` of the lifetime it serves. Elements receive plain property
values, so the same element works in a test, a documentation example or another
host without any store.

## Store core

### API

```js
import { createStore, shallow } from '@canonic2/lattice';

const store = createStore({ muted: false, unread: 3, locked: false });

/** A selection names the fields it reads and derives one value from them. */
const label = {
  fields: ['muted', 'unread'],
  read: (state) => (state.muted ? 'Muted' : `${state.unread} unread`),
};

store.get(); // the current snapshot
store.set({ unread: 4 }); // one commit
store.set((state) => ({ unread: state.unread + 1 }));
store.batch(() => {
  store.set({ muted: true });
  store.set({ unread: 0 });
}); // one commit for both writes
store.watch(label, (text, previous) => render(text), { signal });
```

The core lives in `src/modules/store/` and the package entry point exports
its `createStore` and `shallow`. A store exposes `get`, `set`, `batch` and
`watch` and nothing else. They are standalone functions that do not depend on
`this`, so a capability can hand out `get` and `watch` detached from the store,
as its read-only `state` view does. Test support, such as the selection declaration
check, is exported only from `@canonic2/lattice/testing`; no runtime module
imports it.

### Snapshots

- `createStore(initial, options)` copies `initial` into a snapshot. Its keys
  define the state's shape for the store's lifetime. State fields are string
  keys: an `initial` that is not an object, or that has symbol keys, throws a
  `TypeError`. `options.report` receives
  errors thrown by subscriptions during delivery, as
  [errors](#errors-during-delivery) describes. Its default is the store's
  fallback: the platform's `reportError` where one exists, and otherwise
  rethrowing the error from a microtask so it surfaces as an uncaught exception.
- `get()` returns the current snapshot. The same snapshot is returned until a
  changing write replaces it, so callers may compare snapshots by identity. A
  snapshot never changes after it is returned.
- Snapshots are frozen one level deep (see [decisions](#decisions)). Nested
  values are replaced by a write, never mutated in place; a commit keeps the
  references of every field it did not change.
- State holds data only. Element references, DOM nodes, controllers, timers,
  renderer handles and other resources stay with the code that owns them, never
  in a snapshot.

### Writes and commits

- `set(patch)` takes an object of changed fields, or a function that receives
  the current snapshot and returns one.
- A key that is not in the initial state throws a `TypeError` and changes
  nothing. A misspelled field fails at its first write instead of creating a
  second, unread fact.
- `set` reads each patched value exactly once, and the new snapshot holds the
  values it read, so a getter on a patch cannot make the recorded changed fields
  disagree with the snapshot.
- `set` detects unchanged values before building anything: when every patched
  value is `Object.is`-equal to the current one, it allocates no snapshot and
  notifies no one.
- Otherwise `set` builds one new snapshot, makes it current and records which
  fields changed. Outside a batch, that write is a commit: it is delivered to
  subscribers once.
- `set` throws only when the write itself is rejected, and then changes
  nothing. A write is rejected when:
  - a key is not in the initial state;
  - the patch, or the value a function patch returns, is not an object;
  - reading the patch throws, such as a throwing getter or patch function;
  - `set` is called while another `set` is reading its patch, from a getter or
    a patch function. The outer write would otherwise be built from a snapshot
    the inner one replaced, and both reject with a `TypeError`.

  Errors raised by subscribers during delivery never come out of `set`; see
  [errors](#errors-during-delivery).
- An action that changes several fields commits once, with one patch or one
  `batch`. Commits are synchronous: there is no deferral to a microtask or an
  animation frame.

### Batches

`batch(fn)` groups the writes made by `fn` into one commit:

- `fn` runs synchronously. Each changing `set` inside it creates its own
  snapshot as usual, and `get()` returns it, so later writes see earlier ones and
  every snapshot a caller received stays unchanged. A batch defers delivery, not
  snapshot creation.
- When the outermost `fn` returns, the fields whose final value differs from
  their value before the batch are committed together and delivered once. When
  none differ, nothing is delivered.
- If `fn` throws, the store returns to the snapshot it had when that `batch`
  started, delivers nothing for its writes and rethrows.
- A nested `batch` is a savepoint inside the outer one. Its writes join the
  outer commit when it returns. If it throws, only its own writes roll back, to
  the snapshot at its start; if the outer `fn` catches that error and continues,
  the outer batch goes on with its earlier writes intact and commits normally.
- `fn` must be synchronous. If it returns a promise or any object with a `then`
  method, the batch rolls back as if `fn` had thrown and throws a `TypeError`.
  Writes that the asynchronous continuation makes later are ordinary writes
  outside any batch; an action that needs a write after an `await` makes it as
  a separate commit.
- `batch` called during delivery behaves like `set` called during delivery: its
  commit is queued, as [delivery](#delivery) describes.
- `watch` called while a batch is open throws a `TypeError` and subscribes
  nothing. Its initial `run` would show pending values that a rollback could
  later discard with no delivery to correct them.
- A batch rolls back only for a failure of `fn`. Once a batch commits, a failure
  in a subscriber cannot undo it.

### Selections

A selection is a plain object owned by the capability that defines it:

| Property | Meaning |
| --- | --- |
| `fields` | The state keys `read` depends on; a non-empty array of keys of the store's state |
| `read(state)` | A pure function deriving the selected value from those fields |
| `equals(a, b)` | Optional; compares two results. Defaults to `Object.is`; `shallow` compares two arrays item by item, or two objects with the same prototype by their own enumerable string keys, with `Object.is` |

`fields` is part of the selection's correctness: `read` must not depend on a
field it does not declare, or its subscribers miss updates. The capability that
writes `read` declares `fields` beside it, so a binding never has to know which
fields a value comes from. Tests verify declarations, as [testing](#testing)
describes.

The package exports a `Selection<State, Value>` type describing this shape.
Selections stay plain objects; a capability checks each one with a JSDoc
annotation, which also gives `read` its state type:

```js
/** @satisfies {Selection<CounterState, string>} */
const label = { fields: ['count'], read: (state) => `${state.count} clicks` };
```

Under that annotation a field name that is not a key of the state is a type
error, `read` receives the state type, and its result type is the value a
subscriber's `run` receives. There is no runtime helper for defining
selections.

### Declaration check

`@canonic2/lattice/testing` provides a check that compares a selection's `read`
with its declared `fields`. A test supplies the selection and one or more
representative snapshots, and the check calls `read` with each of them:

- **Undeclared access fails.** If `read` accesses a field that `fields` does not
  declare, in any supplied snapshot, the check fails by throwing an `Error`, as
  [delivery](delivery.md#entry-points) requires of test support.
- **Unobserved declarations do not fail.** The fields accessed across all
  supplied snapshots together are compared with `fields`. A declared field that
  no snapshot led `read` to access is reported as a diagnostic, not a failure:
  conditional branches read different fields, so it indicates incomplete
  coverage or an unnecessary dependency, not a wrong declaration.
- **Recorded access.** Reading a property, including through destructuring, and
  testing a key with `in` count as accessing that field.
- **Unsupported access fails explicitly.** Enumerating the state's keys, such as
  with `Object.keys`, `for…in` or object spread, and reading property
  descriptors make the result depend on the state's whole shape. The check fails
  with an error naming the unsupported access instead of producing a misleading
  result.
- **Diagnostics name their context.** Every failure and diagnostic identifies
  the selection and the supplied snapshot case involved, the unexpected or
  unobserved fields, and the declared fields.
- **Coverage is the test's responsibility.** The check verifies only the
  branches the supplied snapshots reach. A test supplies snapshots that reach
  each branch of `read`; the check cannot prove the absence of dependencies in
  branches it never ran.

**Result.** The check neither logs nor uses callbacks or any other output
channel, and needs no test runner. It throws for undeclared, unknown or
unsupported access. Otherwise it returns a plain report the test inspects:

| Property | Meaning |
| --- | --- |
| `name` | The selection's name, as the test gave it |
| `declared` | The declared fields |
| `cases` | One entry per supplied snapshot: its `name` and the fields `read` `accessed` with it |
| `accessed` | The union of the fields accessed across all cases |
| `unobserved` | Declared fields absent from that union; reporting them never fails the check |

Every list has a deterministic order, so assertions and failure messages stay
stable: `cases` in the own-key order of the supplied cases; `declared`,
`accessed`, `unobserved` and each case's `accessed` in the selection's
declaration order. A failure names the first failing case in that order and
lists its unexpected fields alphabetically.

### Subscriptions

`watch(selection, run, options)` subscribes to a selected value:

- `run(value, previous)` applies it. `run` is called synchronously during
  `watch` with the current value and `previous` set to `undefined`.
- A field in `fields` that is not a key of the state throws a `TypeError` and
  subscribes nothing. `watch` keeps its own copy of `fields`, so changing the
  caller's array later has no effect.
- A `read` that writes to the store during the initial call throws a
  `TypeError` from `watch` and subscribes nothing: its value would describe a
  snapshot the write already replaced, and no delivery would correct it.
- For each commit, a subscription is considered only if one of its `fields`
  changed in that commit. A subscription none of whose fields changed is
  skipped without calling `read`.
- A considered subscription calls `read` with the current snapshot and calls
  `run` only if the result differs from its last value under `equals`.
- `options.signal` is an `AbortSignal`. When it aborts, the subscription ends
  and its functions are released. An already aborted signal subscribes nothing
  and calls nothing. `watch` returns nothing: the signal is the only way to
  unsubscribe, so cleanup follows the lifetime that created the subscription.

### Delivery

- For a commit, considered subscriptions are notified in subscription order,
  synchronously. Each is evaluated at most once per commit.
- A commit made while no delivery is running is delivered before the `set` or
  `batch` that made it returns, together with every commit queued during that
  delivery.
- A commit made during a delivery, by a subscription calling `set` or `batch`,
  does not reenter. Its snapshot becomes current immediately and its delivery is
  queued until every considered subscription of the current commit has been
  notified. The nested `set` or `batch` therefore returns before its own
  subscribers run; they run before the outermost `set` or `batch` returns.
  Queued commits are delivered in the order they were made.
- A considered subscription reads the snapshot current at the moment it is
  notified, so it never observes an older snapshot after a newer one.
- A subscription added during a delivery is not notified for that commit; its
  initial `run` already saw the current snapshot. A subscription removed or
  aborted during a delivery is not called afterwards, including later in the
  same commit and in queued commits.

### Errors during delivery

- An error thrown by a subscription's `read`, `equals` or `run` during delivery
  is passed to the store's `report` option and does not stop delivery: the
  remaining subscriptions are notified. The commit stands.
- `set` and `batch` do not throw delivery errors, including errors from commits
  queued during their delivery. A caller can therefore tell a rejected write,
  which throws and changes nothing, from a committed write, which returns even
  if a subscriber failed.
- A subscription whose callback threw stays subscribed; it compares against the
  last value it delivered successfully.
- An error from the initial `read`, `equals` or `run` inside `watch` propagates
  from `watch` and leaves no subscription.
- `report` is synchronous and does not throw; the code that supplies it is
  responsible for that. If it throws anyway, the store passes that error to its
  fallback, the same one `report` defaults to, and delivery continues. The
  fallback never throws: if the platform's `reportError` throws, it rethrows
  the original error from a microtask instead.

### Cost model

Selectivity has its own cost. Deciding which subscriptions to evaluate must
cost less, in the intended workload, than evaluating them all, and that is
established by measurement, not assumed.

- A write that changes nothing compares each patched value once and allocates
  nothing beyond its patch.
- A changing write allocates one snapshot and records its changed fields,
  inside a batch or not. Unchanged fields keep their references. A batch of
  several changing writes allocates one snapshot per write and delivers once.
- Delivery starts with an in-order walk of the store's subscriptions. For each,
  it intersects the subscription's declared fields with the commit's changed
  fields; that costs up to one check per declared field against the changed
  set. Only subscriptions with a changed field call `read`; only changed results
  call `run`.
- Three questions are kept apart:
  - **Contract:** a subscription none of whose fields changed is not evaluated.
    That is observable through `read` calls and is guaranteed.
  - **Implementation alternatives:** the in-order field walk, or an index of
    subscriptions by field. Either meets the contract; the walk comes first.
  - **Design comparison:** selective delivery against evaluating every `read`.
    A selection with many fields, or a `read` that only returns a primitive
    field, can make the check cost as much as the call it avoids. Evaluating
    everything stays a benchmark baseline, as
    [performance and evidence](performance.md) requires, even though no
    implementation may use it under this contract. If the comparison shows
    selectivity losing materially on the reference workloads, the contract is
    revisited explicitly; the implementation never switches strategy on its
    own and the unfavorable baseline is never dropped.
- Delivery does not copy the subscription collection. Subscriptions added
  during delivery are excluded and removals take effect immediately, as
  [delivery](#delivery) requires, without a per-commit copy.
- A `read` that builds an object or array allocates when it is called, and is
  compared with `shallow`. A `read` that returns a primitive or a reference
  already in state costs neither.
- Nothing renders as a whole. `run` sets the inputs it owns, and elements update
  the DOM affected by those inputs.

This cost suits state that changes at the pace of user actions and service
responses. A store whose subscription count makes the in-order walk the
dominant cost may index subscriptions by field; that needs work-count or timing
evidence that the index saves more than it costs, and must keep the same
ordering and at-most-once guarantees. Values that change on every animation
frame do not belong in a store.

## Writing capability stores

A capability store is a factory. It receives its I/O as injected functions and
returns a read-only view of its state, its selections and its actions:

```js
/**
 * @module Preview settings: whether a preview's links and forms work.
 */
import { createStore } from '@canonic2/lattice';

/**
 * Creates the preview settings capability.
 * @param {object} ports Injected I/O.
 * @param {() => boolean | undefined} ports.read Reads the remembered setting.
 * @param {(checked: boolean) => void} ports.write Remembers the setting.
 * @param {(checked: boolean) => Promise<void>} ports.reload Reloads the preview with it.
 * @returns {PreviewSettings} Its state view, selections and actions.
 */
export function createPreviewSettings({ read, write, reload }) {
  const store = createStore({ actions: read() ?? false, external: false });
  return {
    state: { get: store.get, watch: store.watch },
    select: {
      checked: {
        fields: ['actions', 'external'],
        read: (state) => state.external || state.actions,
      },
      locked: { fields: ['external'], read: (state) => state.external },
    },
    setActions(checked) {
      store.set({ actions: checked });
      write(checked);
      return reload(checked);
    },
    sourceChanged(external) {
      store.set({ external });
    },
  };
}
```

### Capability shape

Capabilities share naming conventions, not a wrapper, base class or runtime
shape check:

| Member | Convention |
| --- | --- |
| `state` | The consumer's read-only view: `get` and `watch`. The store and its `set` and `batch` stay private to the capability. |
| `select` | The capability's selections, each with `fields`, `read` and optional `equals`. |
| Actions | Named methods that express intent, such as `setActions` or `sourceChanged`; the only way state changes. |
| `dispose()` | Present only when the capability owns resources such as timers, pending operations or listeners. Idempotent: a second call does nothing. |

A capability without resources has no `dispose()`; no empty disposal method is
added for uniformity.

### Ownership

- **One owner per fact.** Each fact is one field of one store. Other code reads
  it through a selection or learns about it through an event; it never keeps a
  copy. A requested value and a displayed value are two facts with two fields,
  not one fact in two places.
- **Derived values are selections.** Anything computable from state is a
  selection exported by the owning capability, never a stored field.
- **Actions are the only writers.** A capability's state changes through its
  named actions. Consumers receive the `state` view, which has no `set` or
  `batch`, so no other code can write the store.
- **One store per capability.** A capability that depends on another receives
  that capability's selections and actions, never its store
  object, so dependencies stay explicit and bounded.
- **Application state only.** Selections, settings, registries of sessions or
  documents, and pending or failed operations belong in stores. Focus, hover,
  an open menu, text being typed and a drag in progress stay inside the
  element that owns them.

### Selections

- Declare exactly the fields `read` uses: no fewer, or updates are missed; no
  more, or `read` runs for commits that cannot change its value.
- Prefer a `read` that returns a primitive or a reference already in state, such
  as `{ fields: ['requested'], read: (state) => state.requested }`.
- When an element needs several values, use one `watch` per value, or keep the
  values as one stored object replaced as a unit and select that object.
- Build an object in `read` only when neither is possible, and give the
  selection `equals: shallow`.
- `read` is pure: no I/O, no reading of other stores, no mutation.

### Frequent values

Values that change on every animation frame, such as a pointer position, a
camera during a drag or a scroll offset, stay in the element that produces them.
A store receives the settled value when it matters to the application, such as
the camera when the drag ends.

### Collections

A capability that holds items by key keeps them in a
[keyed collection](collections.md), not in a store field. Selections skip
subscriptions per state field, not per item inside a field: with the items in
one map field replaced immutably, changing one item copied the map and
considered every row's subscriptions, at 1,000 items 2,000 `read`s and about
339 µs per update in the [collection record](../../research/collection.md).
The collection contract owns item storage, identity, batches and delivery; the
capability keeps its scalar state in a store beside it.

## Phases and transitions

A flow with stages has one `phase` field holding a named value. It does not use
a set of booleans whose combinations must be kept consistent by hand.

The rules between phases are a pure function owned by the capability:

```js
transition(state, event) → patch
```

`transition` reads no clock, DOM, network or timer, and returns the fields to
change, or an empty object to ignore the event. Actions turn I/O results, timer
expiry and messages from other documents into events, then apply the returned
patch with `set`. Every product decision about the flow lives in one function
that tests can drive with any sequence of events.

## Asynchronous work

- Every asynchronous operation whose result can arrive after it is no longer
  wanted carries an identity, such as an opaque id or a monotonic sequence
  number owned by the capability. Its result reaches state only through
  `transition`, which ignores results whose identity is no longer current. The
  identity check is what keeps late results out; it is required whether or not
  the operation can be cancelled.
- An operation that holds resources worth releasing early, such as a network
  request, a timer or a subscription, also has an `AbortController`. Work that
  supersedes it aborts that controller, and code after each `await` checks the
  signal before doing more work. Work that cannot be cancelled needs no
  controller.
- An action writes its state first, then performs its side effects, such as
  persisting or reloading. Because `set` throws only for a rejected write,
  a failing subscriber cannot stop those side effects after the state has
  committed.
- Timers belong to the action that starts them and are cleared when their
  operation completes, fails, is superseded or the capability is disposed.
- A capability with operations or timers exposes `dispose()`, which aborts them
  all and is idempotent. The entry point that created the capability calls it
  when that lifetime ends.
- A capability owns recovery from its failures: it records the failure in its
  state where the application shows it, releases the operation's resources and
  applies its own recovery policy, such as retrying or falling back. A failure
  it does not recover from propagates to the caller, as a thrown error or a
  rejected promise. Capabilities do not log, notify or report failures
  themselves; the caller decides how to present them, so the same action can be
  handled differently by a binding, a command or a test, and nothing is reported
  twice.
- A store does not cross a document boundary. Separate documents, such as a
  page and its iframes, each own their stores and exchange messages. Messages
  that report on an operation carry its identity, so a report from a superseded
  operation changes nothing.

## Bindings

A binding connects one element to one capability for one lifetime, through the
subscriptions above and the element's events. [Bindings](bindings.md) owns its
contract: responsibilities, lifetime, ordering, errors, cost and tests.

## Testing

| Subject | Test |
| --- | --- |
| Store core | Unit tests: frozen snapshots that never change after being returned, preserved references of unchanged fields, unknown keys in writes and in `fields`, silent unchanged writes, `equals` and `shallow`, delivery order, subscriptions added, removed or aborted during a delivery, abort before and after subscribing, a throwing initial `read`, `equals` or `run` |
| Nested writes | Unit tests: a `set` from a subscription returns before its own subscribers run, which run before the outermost `set` returns; queued commits are delivered in order; no subscription sees an older snapshot after a newer one |
| Delivery errors | Unit tests: a throwing `read`, `equals` or `run` reaches `report` once, the remaining subscriptions are notified, the commit stands, and `set` returns normally, including for errors in queued commits; a throwing `report` reaches the fallback, including when the platform has no `reportError`, and later writes are still delivered |
| Commits | Unit tests: a multi-field patch notifies each considered subscription once; an action that writes then persists performs its side effects even when a subscriber throws |
| Batches | Unit tests: one delivery per batch; `get()` inside a batch returns the latest write while an earlier snapshot keeps its values; one snapshot per changing write; nothing delivered for an empty net change; a throwing callback restores the prior snapshot and delivers nothing; a nested batch that throws and is caught rolls back only its own writes and the outer batch commits; a callback returning a promise or thenable rolls back and throws `TypeError`; `watch` inside a batch throws `TypeError` and subscribes nothing |
| Selections | Unit tests for every exported selection with the [declaration check](#declaration-check), using snapshots that reach each branch of `read`. Production code never uses the check's recording |
| Declaration check | Unit tests of the check itself: undeclared access fails in any snapshot; unobserved declarations produce a diagnostic, not a failure, judged across all snapshots; property reads, destructuring and `in` are recorded; key enumeration and descriptor reads fail as unsupported; failures name the selection, case, unexpected fields and declared fields |
| Work counts | Unit tests with counters around `read` and `run`: no `read` for a subscription none of whose fields changed, at most one `read` per considered subscription per commit, no `run` for an unchanged result, no snapshot allocated for an unchanged write and exactly one per changing write |
| Transitions | Unit tests driving `transition` with generated event sequences from a fixed seed, asserting the capability's invariants after every event, including late, duplicated and reordered reports |
| Actions | Unit tests with injected fakes: one commit per action, late results with a stale identity ignored, abort on supersession where a controller exists, timers released on completion, failure and `dispose()` |

Generated sequences are reproducible from their seed. A sequence that found a
failure is kept as a fixed regression case beside the generator.

## Decisions

- **A small, dependency-free core of our own.** The package ships without a
  build or runtime dependency, and owning the code fixes notification order,
  equality and error handling exactly as specified. Its size is not a target;
  the machinery each guarantee needs is shown in the architecture before
  implementation.
- **Selections typed by annotation, not a helper.** A `@satisfies` annotation
  with the exported `Selection` type checks fields and types `read` with no
  runtime cost. An identity helper would still need the state type supplied, so
  it would add an export without removing the annotation.
- **Store and selections, not signals.** Named actions and one writer per store
  make each change searchable and testable, and synchronous notification in
  subscription order is easy to follow. Signals with automatic dependency
  tracking were considered; their implicit dependencies and effect scheduling
  hide ordering, and nothing so far shows a need for them.
- **Declared fields, not automatic tracking.** A selection lists the fields it
  reads, so a commit can skip unrelated subscriptions without calling them, with
  no proxies or dependency graph at run time. The declaration is checked in
  tests. Whether skipping saves work depends on the workload: the field checks
  are measured against calling every `read`, as the cost model describes.
- **Synchronous commits; batches roll back, deliveries do not.** An action's
  changes are delivered before the outermost write returns. A failing batch
  callback leaves no partial state, and a nested batch is a savepoint. Once
  committed, state stands even if a subscriber fails. Deferring commits to a
  microtask or frame would delay input feedback and make the current state
  depend on timing; frame-rate work has its own explicitly owned scheduling
  outside the store.
- **A snapshot per write.** Exposing pending batch values through an object that
  changed underneath its readers would break immutability. Each changing write
  creates its own snapshot instead; batches are short, so the extra snapshots
  are a small, bounded cost.
- **Delivery errors go to `report`, not out of `set`.** Throwing subscriber
  errors from `set` would make a committed write look like a rejected one, and
  would cut off an action's side effects after its state had changed. Routing
  them to the store's `report` keeps them observable and keeps `set`'s throw
  meaning "nothing changed".
- **Identity is required, cancellation is optional.** A late result is kept out
  by its identity, which can be as simple as a sequence number. Cancellation
  resources are added only where releasing work early is worth it.
- **Shallow freezing.** Snapshots are frozen one level deep, because an
  accidental in-place mutation would change state without notifying anyone.
  Freezing costs one `Object.freeze` per snapshot and does not protect nested
  values. It stays unless work counts or timings in a named environment show
  it is a material cost; removing it then needs another guard against mutation,
  such as a test-only check.
- **Capability stores, not one application store.** The reason is ownership and
  bounded dependencies: one writer per fact, and each capability reaches only
  what it was given. Performance is not the reason; a single store could also
  subscribe selectively.
- **Elements do not subscribe.** Elements that receive plain inputs stay usable
  in tests, examples and other hosts. Bindings are the only code that knows both
  a store and an element.
- **Measure before optimizing.** Work counts are measured by wrapping `read`
  and `run` in tests; the core has no instrumentation hook. Delivery starts as
  an in-order walk with a field check per subscription. Indexing subscriptions
  by field is added only when counts or timings show the walk dominates, and
  must preserve ordering and at-most-once delivery. Automatic dependency
  tracking needs evidence that a field index is insufficient.
- **Test support outside runtime imports.** The declaration check lives in
  `@canonic2/lattice/testing`, so production pays nothing for it. It checks the
  snapshots a test provides; it cannot prove every possible dependency.
- **The declaration check returns a plain report** (2026-10-24). Tests inspect
  it without logging, callbacks or a test-runner dependency; deterministic
  ordering keeps their assertions and the check's messages stable.
- **Unobserved is a diagnostic, undeclared is a failure.** Reading an
  undeclared field always loses updates, so it fails. A declared field no
  snapshot reached may be a branch the test did not cover, so it is reported
  without failing.
- **Conventions, not a capability wrapper.** A shared vocabulary (`state`,
  `select`, named actions, `dispose()` when resources exist) keeps capabilities
  recognizable without a base class, empty methods or runtime shape validation.
- **Capabilities recover, callers present.** Keeping logging and notification
  out of capabilities avoids coupling them to a presentation channel and avoids
  reporting one failure twice.

## Open questions

- Whether a symbol key in a patch should be rejected. `set` walks string keys
  only, so such a key is ignored. Rejecting it would read the patch's symbol
  keys on every write, an allocation the cost model rules out for unchanged
  writes; the store's own state cannot have symbol keys.
