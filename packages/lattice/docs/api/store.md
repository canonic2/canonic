# Store API

Import from `@canonic2/lattice`.

## `createStore(initial, options?)`

Creates a store with a shallowly frozen copy of `initial`. Its own enumerable
string keys define the state fields.

```js
const task = createStore(
  { text: 'Write the guide', done: false },
  { report: (error) => console.error(error) },
);
```

An initial value that is not an object, is `null`, or has symbol keys throws
a `TypeError`.

`options.report(error)` receives subscriber errors during delivery. It is a
synchronous callback expected not to throw. If it throws, its failure goes to
the fallback reporter. The default fallback uses platform `reportError`,
otherwise rethrows from a microtask; if platform reporting throws, the original
reported error is surfaced from a microtask.

## `store.get()`

Returns the current `Readonly<S>` snapshot. Its identity stays the same until
a changing write replaces it. Unchanged fields retain their references.

Freezing is shallow; nested data must be replaced instead of mutated.

## `store.update(patch)`

Accepts `Partial<S>` or `(state: Readonly<S>) => Partial<S>`.

```js
task.update({ done: true });
task.update((state) => ({ done: !state.done }));
```

Only own enumerable string fields are patched. Each patched value is read once.
Unknown fields, non-object patches, and reentrant writes while another patch
is being resolved throw before publishing a change. Patch-function or getter
errors propagate without publishing a partial write.

Values are compared with `Object.is`. An unchanged write creates no snapshot
and delivers nothing. A changing write replaces the snapshot and delivers
synchronously outside a batch. Subscriber errors do not escape the update.

## `store.batch(fn)`

Runs a synchronous callback and delivers its net field changes as one commit
when the outermost batch completes.

Reads see pending writes. A thrown callback rolls back to that batch's starting
snapshot and rethrows. Nested batches are savepoints. A net-zero batch is silent.

Returning a promise or other thenable rolls back and throws a `TypeError`.
The async operation itself is not cancelled; a later continuation can still make
ordinary writes. Keep asynchronous work outside the batch.

## Selections

`Selection<S, V>` has:

| Member                   | Meaning                                             |
| ------------------------ | --------------------------------------------------- |
| `fields`                 | Non-empty array of state field names                |
| `read(state)`            | Pure derivation returning `V`                       |
| `equals(next, previous)` | Optional result comparison; defaults to `Object.is` |

`read` uses only declared fields and does not mutate state, perform I/O or read
another owner. Lattice copies the field list when subscribing.

## `store.watch(selection, run, options?)`

Calls `run(value, previous)` immediately, then for considered commits whose
selected result differs. `previous` is initially `undefined` and subsequently
the last successfully delivered value. The initial call does not use `equals`.

A commit changing none of the declared fields skips `read`. Considered
subscriptions run in subscription order, at most once per commit.

`options.signal` ends the subscription when aborted. Without it, the
subscription lasts as long as the store. An already-aborted signal subscribes
nothing and calls nothing; field/batch validation can still throw.

`watch` returns `undefined`. Watching inside a batch, unknown or empty fields,
or an initial reader that writes to the store throws. A failure in the initial
reader or callback leaves no subscription. A delivery failure uses `report`,
keeps the subscription and does not advance its last successful value.

Writes made during delivery queue notifications behind the current delivery.
Readers receive the latest state when notified. The outermost write drains
queued notifications before returning.

## `shallow(a, b)`

Returns whether values are equal under `Object.is`, or have equal immediate
contents and the same prototype.

Arrays compare length and indexed values. Objects compare own enumerable
string keys and their values. Values use `Object.is`; nested data is not
compared deeply.

Use this for selections returning plain objects or arrays. It does not compare
a `Map` or `Set` by entries.

## Reader type

`StoreReader<S>` is the read-only type containing `get` and `watch`.
Its methods can be detached.

It adds no runtime wrapper. A capability constructs its own read-only object
when writable methods must be absent at runtime.

**Guide:** [State](../guide/state.md) · [Selections](../guide/selections.md) ·
[Watchers](../guide/watchers.md)
