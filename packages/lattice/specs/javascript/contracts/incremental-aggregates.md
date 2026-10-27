# Incremental aggregates

## Purpose and status

**Status: agreed and implemented in the collection entry point.** Maintain
counts, sums and similarly reversible reductions from committed changes,
without scanning every record on each relevant update. The owner adopts this
API and its product integration following the correctness, ownership, allocation
and bounded shared-host evidence in the [research record](../../../research/incremental-aggregates.md).
The measured revisions and unverified timing/tail limits remain scoped there;
this contract makes no universal speed or precise tail-latency promise.

Ordinary `collection.watch(selection, run, options)` remains available for
arbitrary derivations, order dependencies and live-view reads. Incremental
reduction is explicit and opt-in.

## Public surface

```js
const openCount = {
  fields: ['done'],
  read: (task) => (task.done ? 0 : 1),
  initial: () => 0,
  add: (total, contribution) => total + contribution,
  remove: (total, contribution) => total - contribution,
};

tasks.watchAggregate(
  openCount,
  (value, previous) => {
    summary.open = value;
  },
  { signal },
);
```

`collection.watchAggregate(reduction, run, options)` returning void,
matching `watchItem`/`watch` ownership and callback conventions. Functions remain
safe to detach. `AggregateSelection<Item, Contribution, Value>` has:

| Member                        | Meaning                                                                                 |
| ----------------------------- | --------------------------------------------------------------------------------------- |
| `fields`                      | Item fields used by `read`; copied once. Empty is valid for a constant membership count |
| `read(item)`                  | Pure contribution from one immutable item record, not a collection view                 |
| `initial()`                   | Fresh empty accumulator for each registration or error recovery                         |
| `add(value, contribution)`    | Pure next accumulator including that contribution                                       |
| `remove(value, contribution)` | Pure next accumulator excluding that contribution                                       |
| `equals(a, b)`                | Optional result equality, default `Object.is`; affects delivery, not maintenance        |

Descriptor functions are read once; invalid arguments throw before registration.
Fields are string names, checked like collection-wide dependencies rather than
against an invented shared item schema. JSDoc generic inference must connect
item, contribution, accumulator and callback types through the public entry.
There is no selection constructor, automatic tracking, reducer registry,
independent aggregate store or per-item subscription.

`read` and reducer functions must be deterministic, synchronous and pure. They
must not mutate a previous accumulator, contribution or item, perform I/O, read
undeclared/external changing state or write the collection. Values retained by
the aggregate are immutable by contract; no deep-freeze traversal is introduced.
The five computational callbacks (`initial`, `read`, `add`, `remove`, `equals`)
and validation of their returned values run under the collection write guard,
restored on every exit. `run` retains the existing watcher callback contract
and may queue ordinary writes during delivery.

### Synchronous callback enforcement

Immediately after each computational callback returns, reject an actual Promise
or any other thenable. For non-Promise values, reuse the `isThenable` rule from
`src/report.js`: an object or function with a callable `then` is invalid. Throw a `TypeError`
naming the callback before storing its result, passing it to another reducer
or using it as equality. In particular, a promise from `equals` must never be
treated as a truthy equality result.

For an actual native-branded Promise, including another realm's, attach
discarding fulfillment and rejection handlers using the captured native
`Promise.prototype.then` operation **before** raising the synchronous `TypeError`.
Never await it or accept its settlement as a result. Both handlers return
`undefined` without throwing, so the derived Promise cannot propagate either
the original rejection or a fulfilled value into another unhandled rejection.
Do not read or call a Promise's overridden own `then`; an earlier failed callback
must not leave that Promise's later rejection unhandled. Any such failure still
produces only the phase-specific aggregate error, not a second unhandled rejection.

For arbitrary non-Promise thenables, read `then` once to detect the callable
method, reject synchronously and never invoke/assimilate it. A throwing getter
follows that callback's thrown-error path, preserving its original error/cause.
A non-callable `then` is not a thenable; the result still has to satisfy its
declared type. No deferred aggregate result, global rejection-event handler or
host-specific rejection suppression is introduced. Tests must distinguish real
Promises from objects that merely supply a `then` method.

| Failure phase                                                                              | Outcome for a thrown callback or invalid thenable result                                                                                                                |
| ------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Registration (`initial`, initialization `read`/`add`)                                      | Throw synchronously from `watchAggregate`; discard partial state, leave no subscription or initial delivery, restore the guard                                          |
| Committed maintenance or dirty recovery (`initial`, `read`, `add`, `remove` as applicable) | Keep the model commit, discard the partial candidate, mark/stay dirty and queue one error report for that aggregate/commit; publish nothing; the write returns normally |
| Delivery (`equals`)                                                                        | Report the error, skip `run` for that result and keep the previous successfully delivered value; the healthy accumulator stays current and is not marked dirty          |

The next otherwise relevant commit uses the recovery/equality rules below;
there is no automatic asynchronous retry. Detection does not cancel work the
callback already started or roll back its external effects. The framework
observes an actual Promise's rejection solely to prevent an unhandled rejection;
the callback author still owns its asynchronous work. No existing selection or
batch behavior changes as part of the aggregate callback rule.

### Dependency checking

Reuse `checkSelection` from `@canonic2/lattice/testing` for the item projection.
Pass the existing record-selection shape, `{ fields: reduction.fields,
read: reduction.read }`, with named representative **item records**. Do not use
`checkCollectionSelection`: aggregate `read` receives an item, not a traversal
view. The descriptor variable itself is structurally compatible; selecting the
two members also avoids excess-property diagnostics on an inline reducer object.

For example, the existing checker can inspect this projection independently of
aggregate registration:

```js
import { checkSelection } from '@canonic2/lattice/testing';

checkSelection(
  'openCount.read',
  {
    fields: ['done'],
    read: (task) => (task.done ? 0 : 1),
  },
  {
    open: { done: false },
    closed: { done: true },
  },
);

checkSelection(
  'membershipCount.read',
  {
    fields: [],
    read: () => 1,
  },
  {
    example: { done: false },
  },
);
```

The checker already accepts empty `fields`: `checkCases` copies the declaration
and records access without enforcing a non-empty list. A constant projection
with `fields: []` passes, reporting empty declared/accessed/unobserved lists;
any item-field access with that declaration fails as undeclared. Empty **cases**
still throw. This does not relax runtime `watchItem`'s non-empty-fields rule.
Reads, destructuring and `in` are tracked; enumeration, descriptor access and
mutation retain their existing refusal behavior, including caught refusals.

This checks only synchronous top-level field accesses on supplied branches;
it does not prove purity, asynchronous behavior, reducer laws, arithmetic bounds
or nested-data discipline. Aggregate behavioral tests must cover synchronous
return enforcement and reduction correctness separately. Reuse the checker,
report type and diagnostics; there is no additional aggregate checker.
Product aggregate cases cover empty declarations; the isolated consumer fixture
checks the descriptor's structural compatibility and generic inference.

## Valid reductions and limits

The reducer must be independent of item order, and `remove` must reverse the
effect of `add` for its contribution. Removal may receive a fresh projection of
the same record; reducers must use contribution values rather than depending
on the identity of an object allocated by an earlier `read`. Count, exact integer sum and a `{sum, count}`
accumulator for an average fit this contract. An unfiltered count can already
use collection `size`; no new mechanism is needed merely to read membership size.

For an exact integer sum, each contribution, the initial accumulator and **every
intermediate accumulator value** must remain an integer in the inclusive range
`[-Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER]`. This applies to each
initialization/recovery fold step and every temporary remove-old/add-new step,
not just totals published after commits. A final safe integer does not make an
overflowing intermediate calculation exact. The reducer author must enforce
this restriction (for example with `Number.isSafeInteger` on inputs/results);
the generic framework cannot infer numeric invariants of arbitrary accumulator
types. A guard that throws follows the callback error/recovery rules.

Use integer amounts such as cents only within those bounds. Floating-point
incremental sums can differ from an ordered
full recomputation; this contract promises no bit-identical or drift-free float
sum. Result `equals` does not repair accumulator drift. A float-specific policy
or compensated arithmetic would need a separate reviewed contract.

First/last, sorting, arbitrary joins, an ordinary min/max without removal state,
order-dependent folds and computations depending on other stores remain scanning
selections. A reducer that maintains expensive auxiliary state pays that cost;
the framework does not promise constant-time arbitrary `add` or `remove`.

## Initialization and committed changes

Registration inside an open batch throws, as other collection watchers do.
Validation precedes the aborted-signal check. An already aborted signal causes
no initialization or callback. Otherwise call `initial`, visit each current item
once in current order, project/add each contribution and register the completed
value. Initial `run(value, undefined)` is synchronous and does not call `equals`.
An initialization or initial-delivery error propagates and leaves no subscription.
Check signal liveness during initialization and do not register/publish after abort.

Normal maintenance starts only after a collection commit is irrevocable and
before any user delivery callback for it. The framework consumes immutable
record operands captured for that commit, not the current collection view:

| Committed change                                      | Maintenance                                                                                                       |
| ----------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Relevant field update on a surviving incarnation      | Project before and after; if contributions are `Object.is`-equal, skip reducers; otherwise remove old and add new |
| Unrelated field update                                | No projection, reducer or result delivery                                                                         |
| Insertion                                             | Project/add the record at that commit's boundary                                                                  |
| Removal                                               | Project/remove the removed incarnation's committed contribution                                                   |
| Removal then reuse of a key                           | Remove old incarnation and add new, even when key strings match                                                   |
| Insert then remove a new incarnation within one batch | No net operand                                                                                                    |
| Reorder alone                                         | No maintenance or delivery; these reductions are order-independent                                                |

An updated-then-removed item contributes its pre-batch value to removal; an
inserted-then-updated item contributes its final inserted value. Batched updates
of one surviving item project only pre-batch and final records. Restored fields
and transient items create no aggregate work. The accumulator must match a full
recomputation of each committed state under the reduction's stated arithmetic.

No per-item contribution cache is retained. Project removed/before records from
the commit operands instead. A relevant update costs two projections, trading
that bounded work for no O(items × reductions) cache or incarnation cleanup map.
Initialization is O(items); normal maintenance is proportional to committed
changed operands plus user projection/reducer costs, never a collection scan.

## Batches, reentrancy and delivery

Batch reads keep their existing semantics. Aggregate callbacks observe only
committed results; no aggregate getter or speculative batch value is introduced.
Nested rollback creates no aggregate operands; a caught inner rollback followed
by outer success maintains only the surviving outer changes. Failed/thenable
batches perform no aggregate projection, accumulator change or callback.

Maintain each aggregate once per relevant commit. Queue its resulting immutable
value with the commit. At delivery, compare that value with the last successfully
delivered value and call `run(value, previous)` at most once for that commit.
An unchanged result is silent. Healthy accumulator advancement is independent
of whether `equals` or `run` later succeeds.

Publish in the collection-wide delivery phase after membership and item delivery,
in registration order interleaved with ordinary `watch` subscriptions. Aggregate
calculation happens earlier than this callback phase; existing `watch` reads are
not moved. A callback that queues another commit cannot make an aggregate skip
the earlier commit or read a later live record in its place. Example: a queued
count transition `0 → 1 → 2` must deliver `1` then `2`, not a duplicated latest `2`.

A watcher created during delivery initializes from the current committed state
and records its registration version, skipping already queued commits at or
below that version. Callback writes remain ordinary synchronous reentrant writes
queued through existing delivery. Aborted subscriptions are skipped even when
their computed result is already queued; abort cannot be undone by callback
completion assigning a released value back into subscription state.

## Errors and disposal

For a maintenance projection/reducer error, keep the previous healthy
accumulator, drop partial work, mark this aggregate dirty and queue one report
for that commit. Do not publish a partial or knowingly stale result. The model
commit stands and its write returns normally. Other subscriptions keep running.

**Recovery:** rebuild this aggregate from the current committed state
at the next otherwise relevant commit boundary, before callbacks can write again.
Successful recovery clears dirty state and can publish once. Failed recovery
reports once for that commit and stays dirty. This error-only O(items) scan is
explicit; no missed-delta backlog or unbounded retry queue is retained. Reorder
or unrelated fields alone do not trigger recovery. A caller may abort and
register anew to retry initialization independently.

`equals`/delivery failures use the existing collection reporter and fallback,
leave the computed accumulator current and do not advance the last successfully
delivered value. They do not mark reduction state dirty or undo the model.
Reporter calls occur in delivery after the commit is queued, outside the write
guard; reporting itself may queue later writes without reversing commit order.

The owning `AbortSignal` is the disposal mechanism, as for current watchers.
Abort releases functions, accumulator, last-delivered result and queued private
operands when their delivery boundary ends. It detaches a live signal listener,
is idempotent and never aborts the caller's signal. Without a signal the watcher
lasts with the collection. No per-item owners survive removal, rollback or reuse.

## Implementation boundary and acceptance

The collection maintains aggregates at the irrevocable commit boundary before
user delivery. It captures removed/inserted records only while aggregates are
active. Batched removals use pre-batch records, insertions use final records,
and transient incarnations cancel out. Existing field-change operands are reused.
No public change-stream or per-item subscription is required.

One module-local synchronous-result helper observes native Promise rejection
and rejects asynchronous computational results. The collection module owns
accumulators, dirty recovery, write guarding and committed-result delivery.
Abort clears callback/value ownership; queue draining clears stored results.
There is no full snapshot or per-item contribution cache.

Acceptance must cover exact full-recomputation oracles, net batched work counts,
unrelated skips, order-only skips, removed-after-update/inserted-before-update,
key reuse, nested rollback, reentrant prefix delivery, registration during
delivery, abort in each user callback, partial initialization, dirty recovery,
throwing equality/delivery/reporter and release of private operands. Product coverage lives in `aggregate.test.js`, `aggregate.browser.test.js`,
`synchronous-result.test.js` and the existing collection/bulk suites. Isolated
packed-consumer fixtures exercise runtime delivery, read-only access, generic
inference and invalid declarations.

Synchronous-return cases must exercise promises, object/function thenables,
throwing `then` getters and non-callable `then` values for the applicable
callbacks and phases. Assert no deferred result, correct error routing, guard
restoration and dirty/last-delivered state. Actual Promise cases include already
rejected, fulfilled, later rejecting, cross-realm and own-`then` overrides. Verify
an owned strict-rejection process survives handled violations and that its
unhandled positive control fails. Browser cases observe `unhandledrejection`
without preventing its default behavior. Dependency cases must include empty
declarations with constant and undeclared-field readers through the existing
checker. Exact-sum oracles must exercise safe-range boundaries and a sequence
whose final total is safe but a remove/add intermediate is not; that sequence
must be refused by the example reducer's arithmetic guard, not called exact.

The [research record](../../../research/incremental-aggregates.md) owns measured
costs and their workload/revision limits. Initialization is O(items); ordinary
maintenance is proportional to relevant changed records and user reduction work.
Recovery rescans after an error. Adoption does not establish that every original
quiet-host or tail gate passed, and it schedules no further lab automatically.
